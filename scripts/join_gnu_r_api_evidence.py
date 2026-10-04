#!/usr/bin/env python3
"""Join manifest-bound GNU inventory with package-scoped Rust resolver evidence.

Hashes establish integrity and source identity, not signatures or handler behavior.
The producer of resolver.tsv must run the actual Rust resolver without invoking
handlers. Tests for this importer use explicitly synthetic evidence.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tempfile
from typing import Any

from validate_r_oracle import DEFAULT_MANIFEST, ManifestError, load_manifest, manifest_digest

ROOT = Path(__file__).resolve().parent.parent
SHA256 = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
INTEGER = re.compile(r"(?:-1|0|[1-9][0-9]*)\Z")
INTERFACES = {".C", ".Fortran", ".Call", ".External"}
RESOLVER_COLUMNS = ["dll", "interface", "name", "num_parameters", "resolver_status",
                    "actual_interface", "actual_arity_kind", "actual_num_parameters"]
NATIVE_COLUMNS = ["dll", "dll_path", "interface", "name", "num_parameters",
                  "port_implementation_status", "port_behavior_status", "port_safety_status"]
FUNCTION_COLUMNS = ["package", "package_version", "name", "exported", "local_binding", "type",
                    "is_function", "function_type", "defining_environment", "formals",
                    "documented_args", "inventory_status", "port_implementation_status",
                    "port_behavior_status", "port_safety_status"]

CENSUS_METADATA = {"schema_version", "oracle_source_commit", "oracle_manifest_sha256", "oracle_runtime",
                   "oracle_runtime_sha256", "oracle_identity_validation", "oracle_build_profile",
                   "census_script_sha256", "selected_packages", "namespace_scan_complete",
                   "gnu_api_inventory_complete", "port_compatibility_assessed", "function_bindings",
                   "native_registrations", "issues", "census_exit_code", "files_sha256"}
RESOLVER_METADATA = {"schema_version", "oracle_source_commit", "oracle_manifest_sha256",
                     "census_native_sha256", "census_provenance_sha256", "resolver_tsv_sha256",
                     "source_revision", "source_dirty", "source_files_sha256", "compiled_artifact_sha256",
                     "rust_version", "probe_command", "probe_exit_code", "execution_complete",
                     "probe_log_sha256", "build_profile"}


class EvidenceError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)


def digest(path: Path) -> str:
    require(path.is_file() and not path.is_symlink(), f"not a regular evidence file: {path}")
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def read_json(path: Path) -> dict[str, Any]:
    digest(path)
    value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    require(isinstance(value, dict), f"{path.name} must contain an object")
    return value


def hex_digest(value: Any, label: str) -> str:
    require(isinstance(value, str) and SHA256.fullmatch(value) is not None,
            f"{label} must be a lowercase SHA-256")
    return value


def safe_relative(name: Any, label: str) -> str:
    require(isinstance(name, str) and bool(name) and "\\" not in name,
            f"invalid {label} path")
    path = PurePosixPath(name)
    require(not path.is_absolute() and all(part not in {".", "..", ""} for part in name.split("/")),
            f"invalid {label} path: {name}")
    return name


def verify_files(directory: Path, hashes: Any, label: str, flat: bool = False) -> None:
    require(isinstance(hashes, dict) and bool(hashes), f"{label} hashes must be a nonempty object")
    for name, expected in hashes.items():
        safe_relative(name, label)
        require(not flat or "/" not in name, f"{label} files must be flat")
        path = directory / name
        require(path.resolve().is_relative_to(directory.resolve()), f"{label} path escapes directory")
        require(digest(path) == hex_digest(expected, f"{label} {name}"), f"{label} hash mismatch: {name}")


def table(path: Path, columns: list[str], delimiter: str = ",", expected_hash: str | None = None) -> list[dict[str, str]]:
    require(path.is_file() and not path.is_symlink(), f"not a regular evidence file: {path}")
    data = path.read_bytes()
    if expected_hash is not None:
        require(hashlib.sha256(data).hexdigest() == expected_hash, f"table hash mismatch: {path.name}")
    # Parse the same immutable bytes whose digest was checked, rather than
    # reopening a table that a producer might still be replacing.
    reader = csv.DictReader(io.StringIO(data.decode("utf-8"), newline=""), delimiter=delimiter, strict=True)
    require(reader.fieldnames == columns, f"{path.name} columns must be {columns}")
    result = []
    for number, row in enumerate(reader, 2):
        require(None not in row and all(value is not None for value in row.values()),
                f"malformed {path.name} row {number}")
        require(not any("\x00" in value for value in row.values()), f"NUL in {path.name} row {number}")
        result.append(row)
    return result


def arity(value: str, label: str) -> int:
    require(INTEGER.fullmatch(value) is not None, f"invalid {label} arity: {value!r}")
    parsed = int(value)
    require(parsed <= 2**31 - 1, f"{label} arity exceeds GNU registration integer range")
    return parsed


def key(row: dict[str, str]) -> tuple[str, str, str]:
    require(bool(row["dll"]) and bool(row["name"]), "native key must have dll and name")
    require(row["interface"] in INTERFACES, f"invalid census interface: {row['interface']}")
    return row["dll"], row["interface"], row["name"]


def index(rows: list[dict[str, str]], label: str) -> dict[tuple[str, str, str], dict[str, str]]:
    result = {}
    for row in rows:
        identity = key(row)
        require(identity not in result, f"duplicate {label} key: {identity}")
        arity(row["num_parameters"], label)
        result[identity] = row
    return result


def checked_census(directory: Path, manifest_path: Path) -> tuple[dict[str, Any], dict[str, Any], list, list]:
    manifest = load_manifest(manifest_path)
    provenance = read_json(directory / "provenance.json")
    require(set(provenance) == CENSUS_METADATA, "census provenance keys differ from schema 1")
    require(type(provenance.get("schema_version")) is int and provenance["schema_version"] == 1,
            "census schema_version must be 1")
    require(provenance.get("oracle_source_commit") == manifest["source"]["commit"], "census pinned commit mismatch")
    require(provenance.get("oracle_manifest_sha256") == manifest_digest(manifest_path), "census manifest hash mismatch")
    require(provenance.get("oracle_build_profile") == manifest["build"], "census oracle build profile mismatch")
    require(provenance.get("oracle_identity_validation") == "installation_marker_and_runtime_version",
            "census lacks validated installation identity metadata")
    for field in ["oracle_runtime_sha256", "census_script_sha256"]:
        hex_digest(provenance.get(field), field)
    for field in ["gnu_api_inventory_complete", "port_compatibility_assessed"]:
        require(provenance.get(field) is False, f"census cannot claim {field}")
    require(type(provenance.get("namespace_scan_complete")) is bool, "census scan completeness must be boolean")
    hashes = provenance.get("files_sha256")
    verify_files(directory, hashes, "census", flat=True)
    actual_files = {path.name for path in directory.iterdir() if path.name != "provenance.json"}
    require(actual_files == set(hashes), "census has missing or unrecorded files")
    require({"functions.csv", "native_routines.csv", "packages.csv", "issues.csv"}.issubset(hashes),
            "census required tables are missing")
    issues = table(directory / "issues.csv", ["stage", "item", "severity", "message"], expected_hash=hashes["issues.csv"])
    require(type(provenance["issues"]) is int and provenance["issues"] == len(issues), "census issue count mismatch")
    require(type(provenance["census_exit_code"]) is int, "census exit code must be an integer")
    scan_complete = provenance["census_exit_code"] == 0 and not any(row["severity"] in {"error", "incomplete"} for row in issues)
    require(provenance["namespace_scan_complete"] == scan_complete, "census scan completeness contradicts issues/exit code")
    require(isinstance(provenance["selected_packages"], list)
            and all(isinstance(package, str) and package for package in provenance["selected_packages"])
            and len(set(provenance["selected_packages"])) == len(provenance["selected_packages"]), "invalid selected packages")
    require(isinstance(provenance["oracle_runtime"], str) and bool(provenance["oracle_runtime"]), "census runtime metadata missing")
    native = table(directory / "native_routines.csv", NATIVE_COLUMNS, expected_hash=hashes["native_routines.csv"])
    functions = table(directory / "functions.csv", FUNCTION_COLUMNS, expected_hash=hashes["functions.csv"])
    index(native, "census")
    seen = set()
    for row in functions:
        identity = row["package"], row["name"]
        require(all(identity) and identity not in seen, f"duplicate or empty function key: {identity}")
        require(row["is_function"] == "TRUE", f"nonfunction in functions.csv: {identity}")
        seen.add(identity)
    for rows, field in [(native, "native_registrations"), (functions, "function_bindings")]:
        require(type(provenance.get(field)) is int and provenance[field] == len(rows), f"census {field} count mismatch")
        for row in rows:
            require((row["port_implementation_status"], row["port_behavior_status"], row["port_safety_status"])
                    == ("unclassified", "not_tested", "not_assessed"), "census contains unsupported port claims")
    return manifest, provenance, native, functions


def checked_resolver(directory: Path, census: Path, manifest: dict[str, Any], manifest_path: Path,
                     source_root: Path, artifact: Path | None) -> tuple[dict[str, Any], list]:
    provenance = read_json(directory / "provenance.json")
    require(set(provenance) == RESOLVER_METADATA, "resolver provenance keys differ from schema 1")
    require(type(provenance.get("schema_version")) is int and provenance["schema_version"] == 1,
            "resolver schema_version must be 1")
    expected = {"oracle_source_commit": manifest["source"]["commit"],
                "oracle_manifest_sha256": manifest_digest(manifest_path),
                "census_native_sha256": digest(census / "native_routines.csv"),
                "census_provenance_sha256": digest(census / "provenance.json"),
                "resolver_tsv_sha256": digest(directory / "resolver.tsv"),
                "probe_log_sha256": digest(directory / "probe.log")}
    for field, value in expected.items():
        require(provenance.get(field) == value, f"resolver {field} mismatch")
    require(provenance.get("execution_complete") is True and type(provenance.get("probe_exit_code")) is int
            and provenance["probe_exit_code"] == 0, "resolver probe did not complete successfully")
    require(isinstance(provenance.get("source_revision"), str)
            and COMMIT.fullmatch(provenance["source_revision"]) is not None, "resolver source_revision must be a commit")
    require(type(provenance.get("source_dirty")) is bool, "resolver source_dirty must be boolean")
    for field in ["rust_version"]:
        require(isinstance(provenance.get(field), str) and bool(provenance[field].strip()), f"resolver {field} is missing")
    command = provenance.get("probe_command")
    require(isinstance(command, list) and bool(command) and all(isinstance(arg, str) and arg for arg in command),
            "resolver probe_command must be a nonempty argument list")
    profile = provenance.get("build_profile")
    require(isinstance(profile, dict) and isinstance(profile.get("flags"), list)
            and all(isinstance(arg, str) for arg in profile["flags"])
            and type(profile.get("default_features")) is bool
            and all(isinstance(profile.get(field), str) and profile[field] for field in ["target", "profile"]),
            "resolver build_profile requires flags, default_features, target and profile")
    if "features" in profile:
        require(isinstance(profile["features"], list) and all(isinstance(arg, str) for arg in profile["features"]),
                "resolver features must be a string array")
    sources = provenance.get("source_files_sha256")
    verify_files(source_root, sources, "resolver source")
    required = {"Cargo.toml", "Cargo.lock", "crates/rmath/Cargo.toml"}
    required.update(path.relative_to(source_root).as_posix() for path in (source_root / "crates/rmath").rglob("*.rs"))
    require(required.issubset(sources), "resolver source manifest omits compiled rmath/Cargo inputs")
    expected_artifact = hex_digest(provenance.get("compiled_artifact_sha256"), "compiled artifact")
    if artifact is not None:
        require(digest(artifact) == expected_artifact, "compiled artifact hash mismatch")
    return provenance, table(directory / "resolver.tsv", RESOLVER_COLUMNS, "\t", provenance["resolver_tsv_sha256"])


def reconcile(native: list, resolver: list) -> tuple[list, list]:
    census_rows = index(native, "census")
    resolved_rows = index(resolver, "resolver")
    missing = sorted(census_rows.keys() - resolved_rows.keys())
    extra = sorted(resolved_rows.keys() - census_rows.keys())
    require(not missing and not extra, f"resolver keys differ: missing={missing}; extra={extra}")
    joined, mismatches = [], []
    for identity, original in sorted(census_rows.items()):
        observed = resolved_rows[identity]
        require(observed["num_parameters"] == original["num_parameters"], f"resolver requested arity differs from census: {identity}")
        status = observed["resolver_status"]
        require(status in {"resolved", "unsupported"}, f"invalid resolver status: {status}")
        if status == "unsupported":
            require(not any(observed[field] for field in ["actual_interface", "actual_arity_kind", "actual_num_parameters"]),
                    f"unsupported row contains resolved metadata: {identity}")
            interface_status = arity_status = "not_resolved"
        else:
            require(observed["actual_interface"] in INTERFACES | {".External2"}, f"invalid actual interface: {identity}")
            actual_arity = arity(observed["actual_num_parameters"], "actual")
            require(observed["actual_arity_kind"] == ("variadic" if actual_arity == -1 else "fixed"),
                    f"inconsistent actual arity kind: {identity}")
            if original["interface"] == ".External" and observed["actual_interface"] == ".External2":
                interface_status = "external_family_only"
            else:
                interface_status = "matched" if original["interface"] == observed["actual_interface"] else "mismatch"
            arity_status = "matched" if original["num_parameters"] == observed["actual_num_parameters"] else "mismatch"
        row = {**original, **observed, "interface_status": interface_status, "arity_status": arity_status,
               "port_implementation_status": "unsupported" if status == "unsupported" else "unclassified",
               "port_behavior_status": "not_tested", "port_safety_status": "not_assessed",
               "port_options_status": "not_tested", "port_targets_status": "not_tested"}
        joined.append(row)
        if "mismatch" in {interface_status, arity_status}:
            mismatches.append(row)
    return joined, mismatches


def write_table(path: Path, columns: list[str], rows: list[dict[str, str]], delimiter: str = ",") -> None:
    with path.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=columns, lineterminator="\n", delimiter=delimiter)
        writer.writeheader()
        writer.writerows(rows)


def join(census: Path, resolver: Path, output: Path, manifest_path: Path = DEFAULT_MANIFEST,
         source_root: Path = ROOT, artifact: Path | None = None) -> dict[str, Any]:
    require(not output.is_symlink() and (not output.exists() or (output.is_dir() and not any(output.iterdir()))),
            "output must be a new or empty directory")
    manifest, census_provenance, native, functions = checked_census(census, manifest_path)
    resolver_provenance, resolver_rows = checked_resolver(resolver, census, manifest, manifest_path, source_root, artifact)
    joined, mismatches = reconcile(native, resolver_rows)
    function_rows = [{**row, "port_resolver_status": "not_probed", "port_options_status": "not_tested",
                      "port_targets_status": "not_tested"} for row in sorted(functions, key=lambda row: (row["package"], row["name"]))]
    columns = NATIVE_COLUMNS + [column for column in RESOLVER_COLUMNS if column not in NATIVE_COLUMNS]
    columns += ["interface_status", "arity_status", "port_options_status", "port_targets_status"]
    result = {"schema_version": 1, "joiner_script_sha256": digest(Path(__file__)), "evidence_join_complete": True, "gnu_api_inventory_complete": False,
              "full_gnu_r_parity": False, "behavior_assessed": False, "safety_assessed": False,
              "all_options_tested": False, "all_targets_tested": False,
              "namespace_scan_complete": census_provenance["namespace_scan_complete"],
              "native_rows": len(joined), "resolved_rows": sum(row["resolver_status"] == "resolved" for row in joined),
              "unsupported_rows": sum(row["resolver_status"] == "unsupported" for row in joined),
              "mismatched_rows": len(mismatches), "function_rows_unprobed": len(function_rows),
              "input_sha256": {"census_provenance": digest(census / "provenance.json"),
                               "resolver_provenance": digest(resolver / "provenance.json"),
                               "oracle_manifest": manifest_digest(manifest_path)},
              "census_provenance": census_provenance, "resolver_provenance": resolver_provenance,
              "compiled_artifact_rechecked": artifact is not None,
              "identity_limit": "Content hashes are integrity checks, not signatures or proof of behavior.",
              "external_interface_limit": "GNU .External census groups cannot distinguish .External2 entrypoint semantics."}
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{output.name}-", dir=output.parent) as temporary:
        stage = Path(temporary)
        write_table(stage / "native_routines.csv", columns, joined)
        write_table(stage / "resolver_mismatches.csv", columns, mismatches)
        write_table(stage / "functions.csv", FUNCTION_COLUMNS + ["port_resolver_status", "port_options_status", "port_targets_status"], function_rows)
        (stage / "census_issues.csv").write_bytes((census / "issues.csv").read_bytes())
        require(digest(stage / "census_issues.csv") == census_provenance["files_sha256"]["issues.csv"],
                "census issues changed before publication")
        result["files_sha256"] = {path.name: digest(path) for path in sorted(stage.iterdir())}
        (stage / "provenance.json").write_text(json.dumps(result, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        os.replace(stage, output)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("census", type=Path)
    parser.add_argument("resolver", type=Path)
    parser.add_argument("output", type=Path, help="new or empty joined evidence directory")
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--compiled-artifact", type=Path)
    args = parser.parse_args()
    try:
        result = join(args.census, args.resolver, args.output, args.manifest, args.source_root, args.compiled_artifact)
    except (EvidenceError, ManifestError, OSError, KeyError, csv.Error, json.JSONDecodeError, UnicodeError) as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1
    print(f"Joined {result['native_rows']} native rows; {result['resolved_rows']} resolved, "
          f"{result['unsupported_rows']} unsupported, {result['mismatched_rows']} metadata mismatches. "
          f"{result['function_rows_unprobed']} function bindings remain unprobed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
