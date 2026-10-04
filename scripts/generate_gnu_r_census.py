#!/usr/bin/env python3
"""Run the namespace census with validated pinned-oracle installation metadata."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

from validate_r_oracle import (
    DEFAULT_MANIFEST,
    ManifestError,
    load_manifest,
    manifest_digest,
    verify_runtime,
)


ROOT = Path(__file__).resolve().parent.parent


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def rows(path: Path) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as stream:
        return list(csv.DictReader(stream))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="new or empty census directory")
    parser.add_argument("packages", nargs="*", help="optional selected namespaces")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--runtime", type=Path, help="installed pinned Rscript")
    args = parser.parse_args()
    output = args.output.absolute()
    script = ROOT / "scripts" / "gnu_r_function_census.R"
    try:
        manifest = load_manifest(args.manifest)
        runtime = args.runtime or (
            Path.home() / ".cache" / "rport" / "r-oracle"
            / manifest["source"]["commit"] / "bin" / "Rscript"
        )
        runtime = runtime.absolute()
        verify_runtime(manifest, args.manifest, str(runtime))
        if output.exists() and (not output.is_dir() or any(output.iterdir())):
            raise ManifestError("output must be a new or empty directory")
        result = subprocess.run(
            [str(runtime), "--vanilla", str(script), str(output), *args.packages],
            env={**os.environ, "GNU_R_SOURCE_COMMIT": manifest["source"]["commit"]},
            check=False,
        )
        # An incomplete census deliberately exits 1. Preserve its inventory and
        # issue evidence; neither an incomplete scan nor successful reflection
        # establishes completion of the GNU distribution or of this port.
        required = ["functions.csv", "native_routines.csv", "packages.csv", "issues.csv"]
        if not all((output / name).is_file() for name in required):
            raise ManifestError(f"census did not publish its required tables (exit {result.returncode})")
        issues = rows(output / "issues.csv")
        packages = rows(output / "packages.csv")
        scan_complete = result.returncode == 0 and not any(
            issue["severity"] in {"error", "incomplete"} for issue in issues
        )
        files = {
            path.name: digest(path)
            for path in sorted(output.iterdir())
            if path.is_file()
        }
        provenance = {
            "schema_version": 1,
            "oracle_source_commit": manifest["source"]["commit"],
            "oracle_manifest_sha256": manifest_digest(args.manifest),
            "oracle_runtime": str(runtime),
            "oracle_runtime_sha256": digest(runtime),
            "oracle_identity_validation": "installation_marker_and_runtime_version",
            "oracle_build_profile": manifest["build"],
            "census_script_sha256": digest(script),
            "selected_packages": [item["package"] for item in packages],
            "namespace_scan_complete": scan_complete,
            "gnu_api_inventory_complete": False,
            "port_compatibility_assessed": False,
            "function_bindings": len(rows(output / "functions.csv")),
            "native_registrations": len(rows(output / "native_routines.csv")),
            "issues": len(issues),
            "census_exit_code": result.returncode,
            "files_sha256": files,
        }
        (output / "provenance.json").write_text(
            json.dumps(provenance, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
    except (ManifestError, OSError, KeyError, csv.Error) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(
        f"Pinned census: {provenance['function_bindings']} function bindings, "
        f"{provenance['native_registrations']} native registrations; "
        f"namespace scan {'complete' if scan_complete else 'incomplete'}."
    )
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
