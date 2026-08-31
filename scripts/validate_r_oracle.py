#!/usr/bin/env python3
"""Validate the immutable GNU R oracle manifest and, optionally, its runtime."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = ROOT / "oracle" / "r-oracle.json"
HEX40 = re.compile(r"^[0-9a-f]{40}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


class ManifestError(ValueError):
    pass


def _object(value: Any, name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ManifestError(f"{name} must be an object")
    return value


def _string(value: Any, name: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ManifestError(f"{name} must be a non-empty string")
    return value


def load_manifest(path: Path) -> dict[str, Any]:
    try:
        manifest = _object(json.loads(path.read_text(encoding="utf-8")), "manifest")
    except (OSError, json.JSONDecodeError) as exc:
        raise ManifestError(f"cannot read {path}: {exc}") from exc

    if manifest.get("schema_version") != 1:
        raise ManifestError("schema_version must be 1")

    source = _object(manifest.get("source"), "source")
    runtime = _object(manifest.get("runtime"), "runtime")
    build = _object(manifest.get("build"), "build")
    repository = _string(source.get("repository"), "source.repository")
    commit = _string(source.get("commit"), "source.commit")
    archive_url = _string(source.get("archive_url"), "source.archive_url")
    archive_sha256 = _string(source.get("archive_sha256"), "source.archive_sha256")

    if repository != "https://github.com/wch/r-source.git":
        raise ManifestError("source.repository must be the audited wch/r-source mirror")
    if not HEX40.fullmatch(commit):
        raise ManifestError("source.commit must be an exact lowercase 40-hex commit, not a branch or tag")
    if commit not in archive_url or not archive_url.startswith("https://github.com/wch/r-source/archive/"):
        raise ManifestError("source.archive_url must embed the exact source.commit")
    if not HEX64.fullmatch(archive_sha256):
        raise ManifestError("source.archive_sha256 must be an exact lowercase SHA-256")

    version = _string(runtime.get("version"), "runtime.version")
    if not re.fullmatch(r"\d+\.\d+\.\d+ .+", version):
        raise ManifestError("runtime.version must contain an exact version number and status")
    _string(runtime.get("nickname"), "runtime.nickname")
    revision = runtime.get("svn_revision")
    if not isinstance(revision, int) or isinstance(revision, bool) or revision <= 0:
        raise ManifestError("runtime.svn_revision must be a positive integer")
    source_date = _string(runtime.get("source_date"), "runtime.source_date")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", source_date):
        raise ManifestError("runtime.source_date must be an exact UTC timestamp")
    if _string(build.get("runner_image"), "build.runner_image") != "ubuntu-24.04":
        raise ManifestError("build.runner_image must pin ubuntu-24.04")
    configure_args = build.get("configure_args")
    if not isinstance(configure_args, list) or not configure_args:
        raise ManifestError("build.configure_args must be a non-empty array")
    for index, argument in enumerate(configure_args):
        argument = _string(argument, f"build.configure_args[{index}]")
        if not argument.startswith("--") or any(character.isspace() for character in argument):
            raise ManifestError(f"build.configure_args[{index}] must be one configure argument")
    return manifest


def manifest_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_runtime(manifest: dict[str, Any], manifest_path: Path, rscript: str) -> None:
    marker = Path(rscript).absolute().parent.parent / ".rport-oracle-manifest.sha256"
    expected_digest = manifest_digest(manifest_path)
    try:
        actual_digest = marker.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise ManifestError(
            f"{rscript} has no pinned-oracle provenance marker at {marker}; "
            "install it with scripts/install_r_oracle.sh"
        ) from exc
    if actual_digest != expected_digest:
        raise ManifestError(
            f"oracle provenance marker is {actual_digest!r}, expected manifest digest {expected_digest}"
        )

    expression = (
        "cat(as.character(getRversion()), R.version$status, R.version$nickname, "
        "R.version[['svn rev']], sep='\\n')"
    )
    try:
        result = subprocess.run(
            [rscript, "--vanilla", "-e", expression],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env={**os.environ, "LC_ALL": "C", "LANG": "C"},
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ManifestError(f"cannot query oracle runtime {rscript}: {exc}") from exc
    lines = result.stdout.splitlines()
    expected = manifest["runtime"]
    version_number, status = expected["version"].split(" ", 1)
    expected_lines = [version_number, status, expected["nickname"], str(expected["svn_revision"])]
    if lines != expected_lines:
        raise ManifestError(
            f"runtime identity is {lines!r}, expected {expected_lines!r}"
        )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--runtime", metavar="RSCRIPT", help="also verify an installed pinned Rscript")
    parser.add_argument(
        "--github-output",
        type=Path,
        help="append validated commit/hash/cache metadata to a GitHub Actions output file",
    )
    args = parser.parse_args()
    try:
        manifest = load_manifest(args.manifest)
        if args.runtime:
            verify_runtime(manifest, args.manifest, args.runtime)
        if args.github_output:
            source = manifest["source"]
            cache_dir = Path.home() / ".cache" / "rport" / "r-oracle" / source["commit"]
            with args.github_output.open("a", encoding="utf-8") as output:
                output.write(f"commit={source['commit']}\n")
                output.write(f"archive_sha256={source['archive_sha256']}\n")
                output.write(f"cache_dir={cache_dir}\n")
                output.write(f"manifest_sha256={manifest_digest(args.manifest)}\n")
    except (ManifestError, OSError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(f"Pinned R oracle manifest valid: {manifest['source']['commit']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
