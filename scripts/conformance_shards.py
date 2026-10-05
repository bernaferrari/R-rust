#!/usr/bin/env python3
"""Partition the real parity inventory and verify the complete shard union.

This module does not execute cases or change comparison/timeout rules. Shard
completion and full-corpus completion are deliberately separate properties.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
STATUSES = ("pass", "fail", "xfail", "xpass", "skip")


def digest(value) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def file_digest(path: Path):
    return hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None


def partition(inventory: list[dict], index: int, count: int) -> list[dict]:
    if type(count) is not int or type(index) is not int or count < 1 or not 0 <= index < count or count > len(inventory):
        raise ValueError("shard index/count must select a nonempty partition of the inventory")
    identities = [(row["kind"], row["case"]) for row in inventory]
    if len(identities) != len(set(identities)):
        raise ValueError("duplicate inventory identity")
    return inventory[index::count]


def inventory_at(root: Path) -> list[dict]:
    inventory = []
    for directory, kind in (("cases", "normal"), ("error_cases", "error")):
        inventory.extend({"case": path.stem, "kind": kind}
                         for path in sorted((root / "tests/conformance" / directory).glob("*.R")))
    if not inventory or not any(row["kind"] == "normal" for row in inventory):
        raise ValueError("missing normal conformance inventory")
    return inventory


def source_identity(root: Path) -> dict:
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    paths = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root,
    ).decode().split("\0")
    # Include local dirty/untracked implementation inputs as well as HEAD. CI
    # uses a clean immutable checkout; local candidates cannot borrow its ID.
    inputs = {path: file_digest(root / path) for path in sorted(set(paths)) if path and (
        path.startswith(("crates/", ".cargo/", "vendor/", "tests/conformance/src/"))
        or path in {"Cargo.toml", "Cargo.lock", "scripts/run_parity_case.py"}
        or path.startswith("scripts/conformance")
        or path.endswith("Cargo.toml")
    )}
    return {"source_commit": commit, "source_sha256": digest(inputs)}


def capture_contract(root: Path, inventory: list[dict], *, index: int, count: int,
                     profile: str, timeout: float, strict: bool, pinned: bool,
                     engine: str, cases: Path | None = None, golden: Path | None = None,
                     errors: Path | None = None, error_golden: Path | None = None,
                     xfail: Path | None = None, rustflags: str = "-Awarnings") -> dict:
    selected = partition(inventory, index, count)
    if profile not in {"debug", "release"}:
        raise ValueError("unsupported conformance build profile")
    base = root / "tests/conformance"
    directories = {"normal": (cases or base / "cases", golden or base / "golden"),
                   "error": (errors or base / "error_cases", error_golden or base / "error_golden")}
    inputs = []
    for row in inventory:
        code, expected = directories[row["kind"]]
        inputs.append({**row, "source_sha256": file_digest(code / (row["case"] + ".R")),
                       "golden_sha256": file_digest(expected / (row["case"] + ".out"))})
    return {
        "schema_version": 1, **source_identity(root),
        "corpus_sha256": digest({"cases": inputs, "xfail_sha256": file_digest(xfail or base / "xfail.tsv")}),
        "oracle_manifest_sha256": file_digest(root / "oracle/r-oracle.json"),
        "profile": profile, "build_rustflags": rustflags, "case_timeout_seconds": timeout,
        "strict": strict, "pinned_oracle_required": pinned, "engine_major_minor": engine,
        "shard": {"index": index, "count": count, "global_inventory_total": len(inventory),
                  "selected_total": len(selected), "selected_sha256": digest(selected)},
    }


def rows_from_report(report: dict) -> list[dict]:
    return [row for domain in report["domains"] for row in domain["cases"]]


def merge_reports(root: Path, reports: list[dict], count: int, expected_commit: str) -> dict:
    inventory = inventory_at(root)
    errors, rows, seen_shards = [], [], set()
    expected = capture_contract(root, inventory, index=0, count=count, profile="release",
                                timeout=300.0, strict=True, pinned=True, engine="4.7")
    if expected["source_commit"] != expected_commit:
        errors.append("aggregation checkout does not match the requested commit")
    common = {key: value for key, value in expected.items() if key != "shard"}
    for report in reports:
        try:
            if not isinstance(report, dict) or not isinstance(report.get("execution"), dict):
                raise ValueError("report and execution metadata must be objects")
            contract = report["execution"]
            if {key: value for key, value in contract.items() if key != "shard"} != common:
                raise ValueError("source, corpus, oracle, profile or execution policy differs")
            shard = contract["shard"]
            if not isinstance(shard, dict):
                raise ValueError("shard metadata must be an object")
            index = shard["index"]
            selected = partition(inventory, index, count)
            if shard != {"index": index, "count": count, "global_inventory_total": len(inventory),
                         "selected_total": len(selected), "selected_sha256": digest(selected)}:
                raise ValueError("shard identity does not match the immutable inventory")
            if index in seen_shards:
                raise ValueError(f"duplicate shard {index}")
            seen_shards.add(index)
            shard_rows = rows_from_report(report)
            if any(not isinstance(row, dict) or any(not isinstance(row.get(field), str)
                   for field in ("kind", "case", "status", "detail")) for row in shard_rows):
                raise ValueError(f"malformed case result in shard {index}")
            keys = [(row["kind"], row["case"]) for row in shard_rows]
            allowed = {(row["kind"], row["case"]) for row in selected}
            if len(keys) != len(set(keys)) or not set(keys) <= allowed:
                raise ValueError(f"duplicate or out-of-partition case in shard {index}")
            if any(row["status"] not in STATUSES for row in shard_rows):
                raise ValueError(f"unknown result status in shard {index}")
            if len(shard_rows) != report["total"] or report["inventory_total"] != len(selected):
                raise ValueError(f"inconsistent result counts in shard {index}")
            actual_counts = {status: sum(row["status"] == status for row in shard_rows) for status in STATUSES}
            if report["status_counts"] != actual_counts:
                raise ValueError(f"inconsistent status counts in shard {index}")
            timeout_rows = [row for row in shard_rows if row["detail"].startswith("timeout:")]
            complete = set(keys) == allowed and not timeout_rows
            if report["execution_complete"] != complete:
                raise ValueError(f"incorrect completion claim in shard {index}")
            rows.extend(shard_rows)
            if not complete:
                errors.append(f"shard {index} did not complete its selected cases")
        except (KeyError, TypeError, ValueError) as exc:
            errors.append(f"invalid shard report: {exc}")
    missing_shards = sorted(set(range(count)) - seen_shards)
    if missing_shards:
        errors.append(f"missing shards: {missing_shards}")
    attempted = {(row["kind"], row["case"]) for row in rows}
    unattempted = [row for row in inventory if (row["kind"], row["case"]) not in attempted]
    status_counts = {status: sum(row["status"] == status for row in rows) for status in STATUSES}
    timed_out = sum(row["detail"].startswith("timeout:") for row in rows)
    complete = not errors and not unattempted and not timed_out and len(rows) == len(inventory)
    return {
        "execution": common, "shard_count": count, "received_shards": sorted(seen_shards),
        "inventory_total": len(inventory), "total": len(rows), "status_counts": status_counts,
        "timed_out": timed_out, "unattempted_cases": unattempted,
        "execution_complete": complete, "full_inventory_complete": complete,
        "strict_pass": complete and status_counts["pass"] == len(inventory),
        "errors": errors, "cases": sorted(rows, key=lambda row: (row["kind"], row["case"])),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--shard-count", type=int, required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--json", type=Path, required=True)
    parser.add_argument("--markdown", type=Path, required=True)
    args = parser.parse_args()
    reports, read_errors = [], []
    for path in sorted(args.input_dir.glob("*/summary.json")):
        try:
            reports.append(json.loads(path.read_text()))
        except (OSError, json.JSONDecodeError) as exc:
            read_errors.append(f"cannot read {path}: {exc}")
    try:
        report = merge_reports(args.root, reports, args.shard_count, args.source_commit)
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        report = {"strict_pass": False, "execution_complete": False, "errors": [str(exc)]}
    if read_errors:
        report["errors"].extend(read_errors)
        report.update(strict_pass=False, execution_complete=False, full_inventory_complete=False)
    args.json.parent.mkdir(parents=True, exist_ok=True)
    args.json.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    args.markdown.parent.mkdir(parents=True, exist_ok=True)
    lines = ["# Exact-oracle conformance coverage", "",
             f"Full inventory complete: **{report['execution_complete']}**",
             f"Strict parity passed: **{report['strict_pass']}**", "",
             f"Results: {report.get('total', 0)}/{report.get('inventory_total', 0)}", ""]
    lines.extend(f"- {error}" for error in report["errors"])
    for row in report.get("cases", []):
        if row["status"] != "pass":
            lines.append(f"- {row['kind']}/{row['case']}: {row['status']} {row['detail']}")
    args.markdown.write_text("\n".join(lines) + "\n")
    print(f"Conformance union: {report.get('total', 0)}/{report.get('inventory_total', 0)}; strict pass={report['strict_pass']}")
    return 0 if report["strict_pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
