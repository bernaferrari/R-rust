#!/usr/bin/env python3
"""Validate the immutable GNU R test inventory and its disposition ledger."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import NamedTuple


BEAD_PATTERN = re.compile(r"^rport-[a-z0-9]+(?:\.[0-9]+)*$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
DISPOSITIONS = frozenset({"pass", "xfail", "skip"})


class CorpusError(ValueError):
    pass


class Entry(NamedTuple):
    path: str
    digest: str
    disposition: str
    owner: str
    reason: str


class Report(NamedTuple):
    entries: tuple[Entry, ...]
    imported_files: int
    total: int
    passing: int
    expected_failures: int
    skipped: int
    runnable: int


def _read_rows(path: Path, columns: int) -> list[tuple[int, list[str]]]:
    if not path.is_file():
        raise CorpusError(f"missing {path.name}: {path}")
    rows: list[tuple[int, list[str]]] = []
    for line_number, raw_line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if not raw_line or raw_line.startswith("#"):
            continue
        fields = raw_line.split("\t")
        if len(fields) != columns:
            raise CorpusError(
                f"{path.name}:{line_number}: expected {columns} tab-separated columns"
            )
        rows.append((line_number, fields))
    return rows


def _validate_relative_path(path: str, source: str) -> PurePosixPath:
    parsed = PurePosixPath(path)
    if (
        parsed.is_absolute()
        or ".." in parsed.parts
        or path != parsed.as_posix()
        or path in {"", "."}
    ):
        raise CorpusError(f"{source}: invalid relative path {path!r}")
    return parsed


def _load_inventory(corpus: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    previous = ""
    for line_number, fields in _read_rows(corpus / "inventory.tsv", 2):
        path, digest = fields
        _validate_relative_path(path, f"inventory.tsv:{line_number}")
        if path in result:
            raise CorpusError(f"inventory.tsv:{line_number}: duplicate path {path}")
        if path <= previous:
            raise CorpusError("inventory.tsv must be sorted by path")
        if not SHA256_PATTERN.fullmatch(digest):
            raise CorpusError(f"inventory.tsv:{line_number}: invalid SHA-256 for {path}")
        result[path] = digest
        previous = path
    if not result:
        raise CorpusError("inventory.tsv has no test files")
    return result


def _load_dispositions(corpus: Path) -> dict[str, tuple[str, str, str]]:
    result: dict[str, tuple[str, str, str]] = {}
    previous = ""
    for line_number, fields in _read_rows(corpus / "dispositions.tsv", 4):
        path, disposition, owner, reason = fields
        parsed = _validate_relative_path(path, f"dispositions.tsv:{line_number}")
        if len(parsed.parts) != 1 or parsed.suffix not in {".R", ".Rin"}:
            raise CorpusError(
                f"dispositions.tsv:{line_number}: only top-level .R/.Rin drivers "
                "may have dispositions"
            )
        if path in result:
            raise CorpusError(f"dispositions.tsv:{line_number}: duplicate path {path}")
        if path <= previous:
            raise CorpusError("dispositions.tsv must be sorted by path")
        if disposition not in DISPOSITIONS:
            raise CorpusError(
                f"dispositions.tsv:{line_number}: invalid disposition {disposition!r}"
            )
        if disposition == "pass":
            if owner != "-" or reason != "-":
                raise CorpusError(
                    f"dispositions.tsv:{line_number}: pass must use '-' owner and reason"
                )
        else:
            if not BEAD_PATTERN.fullmatch(owner):
                raise CorpusError(
                    f"dispositions.tsv:{line_number}: {disposition} requires an owner bead"
                )
            if not reason.strip() or reason == "-":
                raise CorpusError(
                    f"dispositions.tsv:{line_number}: {disposition} requires a reason"
                )
        result[path] = (disposition, owner, reason)
        previous = path
    return result


def _validate_source_commit(corpus: Path, expected_commit: str) -> None:
    prefix = "# r-source commit\t"
    for line in (corpus / "inventory.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith(prefix):
            actual = line.removeprefix(prefix)
            if actual != expected_commit:
                raise CorpusError(
                    f"inventory.tsv pins r-source {actual}, expected {expected_commit}"
                )
            return
    raise CorpusError("inventory.tsv is missing its pinned r-source commit")


def validate_corpus(corpus: Path, expected_commit: str | None = None) -> Report:
    corpus = corpus.resolve()
    inventory = _load_inventory(corpus)
    dispositions = _load_dispositions(corpus)
    if expected_commit is not None:
        _validate_source_commit(corpus, expected_commit)

    inventory_paths = set(inventory)
    disposition_paths = set(dispositions)
    drivers = {
        path
        for path in inventory_paths
        if len(PurePosixPath(path).parts) == 1
        and PurePosixPath(path).suffix in {".R", ".Rin"}
    }
    missing = sorted(drivers - disposition_paths)
    unknown = sorted(disposition_paths - inventory_paths)
    if missing:
        raise CorpusError(f"missing dispositions for: {', '.join(missing)}")
    if unknown:
        raise CorpusError(f"dispositions not in inventory.tsv: {', '.join(unknown)}")

    vendor = corpus / "vendor"
    actual_paths = (
        {
            path.relative_to(vendor).as_posix()
            for path in vendor.rglob("*")
            if path.is_file()
        }
        if vendor.is_dir()
        else set()
    )
    unlisted = sorted(actual_paths - inventory_paths)
    absent = sorted(inventory_paths - actual_paths)
    if unlisted:
        raise CorpusError(f"vendor files not in inventory.tsv: {', '.join(unlisted)}")
    if absent:
        raise CorpusError(f"inventory files missing from vendor: {', '.join(absent)}")

    entries: list[Entry] = []
    for relative_path, expected_digest in inventory.items():
        case_path = vendor / relative_path
        actual_digest = hashlib.sha256(case_path.read_bytes()).hexdigest()
        if actual_digest != expected_digest:
            raise CorpusError(
                f"checksum mismatch for {relative_path}: "
                f"got {actual_digest}, expected {expected_digest}"
            )
        if relative_path in dispositions:
            disposition, owner, reason = dispositions[relative_path]
            entries.append(
                Entry(relative_path, expected_digest, disposition, owner, reason)
            )

    passing = sum(entry.disposition == "pass" for entry in entries)
    expected_failures = sum(entry.disposition == "xfail" for entry in entries)
    skipped = sum(entry.disposition == "skip" for entry in entries)
    return Report(
        tuple(entries),
        len(inventory),
        len(entries),
        passing,
        expected_failures,
        skipped,
        passing + expected_failures,
    )


def _write_markdown(report: Report, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        "# Pinned GNU R Test Inventory",
        "",
        f"Imported files: **{report.imported_files}**; test drivers: **{report.total}**; "
        f"runnable: **{report.runnable}**; "
        f"expected failures: **{report.expected_failures}**; skipped: **{report.skipped}**.",
        "",
        "| Upstream test | Disposition | Owner | Reason |",
        "| --- | --- | --- | --- |",
    ]
    lines.extend(
        f"| `{entry.path}` | {entry.disposition} | `{entry.owner}` | {entry.reason} |"
        for entry in report.entries
    )
    destination.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--corpus", type=Path, default=root / "tests" / "upstream-r"
    )
    parser.add_argument("--oracle", type=Path, default=root / "oracle" / "r-oracle.json")
    parser.add_argument("--markdown", type=Path)
    args = parser.parse_args(argv)

    try:
        oracle = json.loads(args.oracle.read_text(encoding="utf-8"))
        expected_commit = oracle["source"]["commit"]
        report = validate_corpus(args.corpus, expected_commit)
        if args.markdown is not None:
            _write_markdown(report, args.markdown)
    except (CorpusError, KeyError, OSError, json.JSONDecodeError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1

    print(
        f"Pinned upstream corpus OK: {report.imported_files} files, "
        f"{report.total} test drivers; "
        f"{report.runnable} runnable, {report.expected_failures} xfail, "
        f"{report.skipped} skipped."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
