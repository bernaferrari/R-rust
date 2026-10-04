#!/usr/bin/env python3
"""Reproduce portable datasets assets from the authenticated pinned GNU R."""
from __future__ import annotations

import argparse
import csv
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[5]


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rows(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as stream:
        return list(csv.DictReader(stream, delimiter="\t"))


def validate_inventory(objects: list[dict[str, str]], topics: dict[str, list[str]],
                       index: list[dict[str, str]], database_size: int) -> None:
    names = {row["name"] for row in objects}
    topic_objects = sum(topics.values(), [])
    if len(objects) != 108 or len(names) != 108 or len(topics) != 91 or len(index) != 108:
        raise ValueError("unexpected pinned inventory shape")
    if len(topic_objects) != 108 or set(topic_objects) != names:
        raise ValueError("incomplete or duplicate dataset topic inventory")
    if len({row["item"] for row in index}) != 108:
        raise ValueError("duplicate dataset index row")
    end = 0
    for row in sorted(objects, key=lambda row: int(row["offset"])):
        offset, count = int(row["offset"]), int(row["bytes"])
        if offset != end or count <= 0 or offset + count > database_size:
            raise ValueError("invalid dataset key or incomplete database coverage")
        end = offset + count
    if end != database_size:
        raise ValueError("dataset keys do not cover the complete pinned database")


def generate(rscript: Path, manifest: Path, output: Path) -> None:
    spec = importlib.util.spec_from_file_location(
        "validate_r_oracle", ROOT / "scripts/validate_r_oracle.py"
    )
    assert spec and spec.loader
    validator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(validator)
    pinned = validator.load_manifest(manifest)
    validator.verify_runtime(pinned, manifest, str(rscript))
    if output.exists() and any(output.iterdir()):
        raise ValueError("output must be new or empty")
    output.mkdir(parents=True, exist_ok=True)
    script = Path(__file__).with_suffix(".R")
    with tempfile.TemporaryDirectory(prefix="rport-datasets-") as temporary:
        scratch = Path(temporary)
        result = subprocess.run(
            [str(rscript), "--vanilla", str(script), str(scratch)],
            capture_output=True, text=True, timeout=120, check=False,
            env={**os.environ, "LC_ALL": "C", "LANG": "C"},
        )
        (output / "generation.log").write_text(
            result.stdout + result.stderr, encoding="utf-8"
        )
        if result.returncode or result.stdout.splitlines() != [
            "Pinned datasets export complete: 108 objects, 91 topics, 108 index rows"
        ]:
            raise ValueError("pinned export did not complete; see generation.log")
        package = Path(rows(scratch / "package.tsv")[0]["package"])
        originals = ["DESCRIPTION", "NAMESPACE", "Meta/data.rds",
                     "data/Rdata.rdb", "data/Rdata.rdx", "data/Rdata.rds"]
        objects = rows(scratch / "objects.tsv")
        topics: dict[str, list[str]] = {}
        for row in rows(scratch / "topics.tsv"):
            topics.setdefault(row["topic"], []).append(row["object"])
        index = rows(scratch / "index.tsv")
        validate_inventory(objects, topics, index, (scratch / "Rdata.rdb").stat().st_size)
        metadata = {
            "schema_version": 1,
            "oracle_manifest_sha256": digest(manifest),
            "source": pinned["source"],
            "runtime": pinned["runtime"],
            "installed_files_sha256": {name: digest(package / name) for name in originals},
            "generator_sha256": {
                "generate.py": digest(Path(__file__)), "generate.R": digest(script)
            },
            "serialization_version": 2,
            "lazy_database_compression": 3,
            "lazy_database_references": 0,
            "namespace_exports": [],
            "objects": objects,
            "topics": topics,
            "index": index,
            "artifacts_sha256": {},
        }
        for name in ("all.rda", "values.rds", "Rdata.rdb", "Rdata.rdx", "Rdata.rds", "data-index.rds"):
            shutil.copyfile(scratch / name, output / name)
            metadata["artifacts_sha256"][name] = digest(output / name)
        for name in ("DESCRIPTION", "NAMESPACE"):
            shutil.copyfile(package / name, output / name)
            metadata["artifacts_sha256"][name] = digest(output / name)
        fields = ["name", "type", "length", "class", "dimensions", "attributes"]
        lines = ["\t".join(fields)]
        for row in objects:
            values = [row[field] for field in fields]
            if any("\t" in value or "\n" in value for value in values):
                raise ValueError("object contract contains a TSV delimiter")
            lines.append("\t".join(values))
        contract = output / "object-contracts.tsv"
        contract.write_text("\n".join(lines) + "\n", encoding="utf-8")
        metadata["artifacts_sha256"][contract.name] = digest(contract)
        (output / "inventory.json").write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--rscript", required=True, type=Path)
    parser.add_argument("--manifest", type=Path, default=ROOT / "oracle/r-oracle.json")
    args = parser.parse_args()
    generate(args.rscript, args.manifest, args.output)


if __name__ == "__main__":
    main()
