#!/usr/bin/env python3
"""Compare complete strict conformance receipts and list changed case verdicts."""

import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess

from conformance_shards import ROOT, STATUSES

CASE_DIRECTORIES = {
    "cases": "normal",
    "golden": "normal",
    "golden_utf8": "normal",
    "error_cases": "error",
    "error_golden": "error",
    "error_golden_utf8": "error",
}


def read_receipt(path):
    report = json.loads(path.read_text())
    execution = report["execution"]
    if (
        not report["full_inventory_complete"]
        or report["errors"]
        or report["timed_out"]
        or report["unattempted_cases"]
        or not execution["strict"]
        or not execution["pinned_oracle_required"]
    ):
        raise ValueError(f"{path} must be a complete, pinned strict conformance union")
    cases = {(row["kind"], row["case"]): row["status"] for row in report["cases"]}
    counts = Counter(cases.values())
    if (
        len(cases) != len(report["cases"])
        or len(cases) != report["inventory_total"]
        or set(counts) - set(STATUSES)
        or dict.fromkeys(STATUSES, 0) | counts != report["status_counts"]
    ):
        raise ValueError(f"{path} has inconsistent case totals")
    return execution, cases


def compare(before_path, after_path):
    before, old = read_receipt(before_path)
    after, new = read_receipt(after_path)
    provenance = {"source_commit", "source_sha256", "corpus_sha256"}
    profile = {key: value for key, value in before.items() if key not in provenance}
    if profile != {key: value for key, value in after.items() if key not in provenance}:
        raise ValueError("receipts have different oracle or execution settings")

    changed_paths = subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", "-z",
         before["source_commit"], after["source_commit"], "--", "tests/conformance"],
        cwd=ROOT, text=True,
    ).rstrip("\0").split("\0")
    if "tests/conformance/xfail.tsv" in changed_paths:
        raise ValueError("expected-failure rules changed between receipts")
    edited = set()
    for changed in changed_paths:
        path = Path(changed)
        kind = CASE_DIRECTORIES.get(path.parent.name)
        if kind and path.suffix in {".R", ".out"}:
            edited.add((kind, path.stem))

    def outcome(key):
        kind, case = key
        return {
            "kind": kind,
            "case": case,
            "before": old.get(key),
            "after": new.get(key),
        }

    shared = old.keys() & new.keys()
    return {
        "before": before,
        "after": after,
        "before_counts": Counter(old.values()),
        "after_counts": Counter(new.values()),
        "added": [outcome(key) for key in sorted(new.keys() - old.keys())],
        "removed": [outcome(key) for key in sorted(old.keys() - new.keys())],
        "edited": [outcome(key) for key in sorted(shared & edited)],
        "verdict_changes": [
            outcome(key) for key in sorted(shared - edited) if old[key] != new[key]
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(compare(args.before, args.after), indent=2))
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Cannot compare receipts: {error}\n")


if __name__ == "__main__":
    main()
