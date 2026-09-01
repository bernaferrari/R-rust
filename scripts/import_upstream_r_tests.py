#!/usr/bin/env python3
"""Import GNU R's complete tests tree from the pinned oracle source archive."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, default=root / "oracle" / "r-oracle.json")
    parser.add_argument("--output", type=Path, default=root / "tests" / "upstream-r")
    args = parser.parse_args()

    oracle = json.loads(args.oracle.read_text(encoding="utf-8"))
    source = oracle["source"]
    commit = source["commit"]
    expected_archive_digest = source["archive_sha256"]
    actual_archive_digest = hashlib.sha256(args.archive.read_bytes()).hexdigest()
    if actual_archive_digest != expected_archive_digest:
        raise SystemExit(
            f"archive SHA-256 is {actual_archive_digest}, expected {expected_archive_digest}"
        )

    archive_prefix = f"r-source-{commit}/tests/"
    imported: dict[str, bytes] = {}
    with tarfile.open(args.archive, mode="r:gz") as archive:
        for member in archive.getmembers():
            if not member.isfile() or not member.name.startswith(archive_prefix):
                continue
            relative = member.name.removeprefix(archive_prefix)
            parsed = PurePosixPath(relative)
            if parsed.is_absolute() or ".." in parsed.parts or relative != parsed.as_posix():
                raise SystemExit(f"unsafe archive member {member.name}")
            extracted = archive.extractfile(member)
            if extracted is None:
                raise SystemExit(f"could not read {member.name} from archive")
            imported[relative] = extracted.read()

    if not imported:
        raise SystemExit("archive contains no r-source/tests files")

    vendor = args.output / "vendor"
    if vendor.exists():
        if vendor.name != "vendor":
            raise SystemExit(f"refusing to replace unexpected output directory {vendor}")
        shutil.rmtree(vendor)
    vendor.mkdir(parents=True, exist_ok=True)
    for relative, contents in sorted(imported.items()):
        destination = vendor / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(contents)

    inventory_lines = [
        f"# r-source commit\t{commit}",
        f"# archive sha256\t{expected_archive_digest}",
        "# path\tsha256",
    ]
    inventory_lines.extend(
        f"{relative}\t{hashlib.sha256(contents).hexdigest()}"
        for relative, contents in sorted(imported.items())
    )
    (args.output / "inventory.tsv").write_text(
        "\n".join(inventory_lines) + "\n", encoding="utf-8"
    )
    print(f"Imported {len(imported)} pinned GNU R test-tree files into {vendor}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
