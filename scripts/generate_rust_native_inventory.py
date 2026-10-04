#!/usr/bin/env python3
"""Build and run the actual, non-invoking Rust native registration exporter."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tomllib

import join_gnu_r_api_evidence as evidence
from run_parity_case import positive_seconds

ROOT = Path(__file__).resolve().parent.parent
EXPORTER = "mainutils::dotcode::native_inventory::export_native_registration_inventory"
FOOTER = re.compile(r"^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; \d+ filtered out; finished in [0-9.]+s$", re.MULTILINE)
COUNT = re.compile(r"Exported (\d+) native registration descriptors without invocation")


class InventoryError(ValueError):
    def __init__(self, message: str, exit_code: int = 1):
        super().__init__(message)
        self.exit_code = exit_code


def require(condition: bool, message: str) -> None:
    if not condition:
        raise InventoryError(message)


def inspect(command: list[str], source: Path, environment: dict[str, str]) -> str:
    try:
        result = subprocess.run(command, cwd=source, env=environment, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=False)
    except subprocess.TimeoutExpired as error:
        raise InventoryError(f"metadata command timed out: {command[0]}") from error
    require(result.returncode == 0, f"metadata command failed: {command[0]}: {result.stderr.strip()}")
    return result.stdout.strip()


def git_identity(source: Path, environment: dict[str, str]) -> tuple[str, bool]:
    revision = inspect(["git", "rev-parse", "HEAD"], source, environment)
    require(evidence.COMMIT.fullmatch(revision) is not None, "source revision must be an exact commit")
    status = inspect(["git", "status", "--porcelain", "--untracked-files=all", "--", "Cargo.toml", "Cargo.lock",
                      ".cargo", "rust-toolchain", "rust-toolchain.toml", "crates/rmath"], source, environment)
    return revision, bool(status)


def source_files(source: Path) -> list[Path]:
    required = [source / name for name in ["Cargo.toml", "Cargo.lock", "crates/rmath/Cargo.toml"]]
    required += list((source / "crates/rmath").rglob("*.rs"))
    for name in [".cargo/config.toml", ".cargo/config", "rust-toolchain.toml", "rust-toolchain"]:
        if (source / name).exists():
            required.append(source / name)
    require(any(path.suffix == ".rs" for path in required), "source tree has no rmath Rust inputs")
    for path in required:
        require(path.resolve().is_relative_to(source), "compiled source path escapes source root")
        evidence.digest(path)
    return sorted(set(required))


def snapshot(source: Path) -> dict[str, bytes]:
    return {path.relative_to(source).as_posix(): path.read_bytes() for path in source_files(source)}


def hashes(contents: dict[str, bytes]) -> dict[str, str]:
    return {name: hashlib.sha256(data).hexdigest() for name, data in contents.items()}


def configuration_files(source: Path, environment: dict[str, str]) -> list[Path]:
    cargo_home = Path(environment.get("CARGO_HOME", str(Path.home() / ".cargo")))
    candidates = []
    for directory in [source, *source.parents]:
        candidates.extend([directory / ".cargo/config", directory / ".cargo/config.toml"])
    candidates.extend([cargo_home / "config", cargo_home / "config.toml"])
    return list(dict.fromkeys(path for path in candidates if path.is_file()))


def target(source: Path, environment: dict[str, str], rust_info: str) -> str:
    if environment.get("CARGO_BUILD_TARGET"):
        return environment["CARGO_BUILD_TARGET"]
    # Cargo's nearest workspace configuration takes precedence over ancestors.
    # A legacy config takes precedence if both config names exist.
    for path in configuration_files(source, environment):
        configuration = tomllib.loads(path.read_text(encoding="utf-8"))
        selected = configuration.get("build", {}).get("target")
        if selected is not None:
            if isinstance(selected, list):
                require(len(selected) == 1, "native inventory requires a single configured target")
                selected = selected[0]
            require(isinstance(selected, str) and bool(selected), "configured Cargo target is invalid")
            return selected
    host = re.search(r"^host: (.+)$", rust_info, re.MULTILINE)
    require(host is not None, "rustc did not report its host target")
    return host.group(1)


def compiler_artifact(log: str) -> dict:
    artifacts = []
    for line in log.splitlines():
        if not line.startswith("{"):
            continue
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(item, dict) or item.get("reason") != "compiler-artifact":
            continue
        target_data, profile = item.get("target"), item.get("profile")
        require(isinstance(target_data, dict) and isinstance(profile, dict), "malformed compiler artifact metadata")
        if target_data.get("name") == "rmath" and profile.get("test") is True and item.get("executable"):
            require(isinstance(item["executable"], str), "compiler artifact executable path is invalid")
            artifacts.append(item)
    require(len(artifacts) == 1, "probe log must identify exactly one rmath test compiler artifact")
    return artifacts[0]


def generate(census: Path, output: Path, source_root: Path = ROOT, manifest_path: Path = evidence.DEFAULT_MANIFEST,
             target_dir: Path | None = None, timeout: float = 600, environment: dict[str, str] | None = None) -> dict:
    require(math.isfinite(timeout) and timeout > 0, "timeout must be positive and finite")
    require(os.name == "posix", "native inventory deadline runner requires POSIX")
    source = source_root.resolve()
    output = output.absolute()
    require(not output.is_symlink() and (not output.exists() or (output.is_dir() and not any(output.iterdir()))),
            "output must be a new or empty directory")
    require(not output.resolve().is_relative_to(source / "crates/rmath"), "output cannot be inside compiled rmath sources")
    manifest, census_provenance, native, _ = evidence.checked_census(census, manifest_path)
    input_identity = (evidence.digest(census / "provenance.json"), evidence.manifest_digest(manifest_path))
    require(bool(native), "native census is empty; exporter cannot establish coverage")
    for row in native:
        require(not any(character in row[field] for field in ["dll", "interface", "name", "num_parameters"]
                        for character in ['\t', '\r', '\n', '"']), "native key cannot be represented by exporter's raw TSV framing")
    env = dict(os.environ if environment is None else environment)
    if target_dir is not None:
        env["CARGO_TARGET_DIR"] = str(target_dir.absolute())
    wrapper = source / "scripts/cargo_dev.sh"
    helper = ROOT / "scripts/run_parity_case.py"
    wrapper_digest = evidence.digest(wrapper)
    helper_digest = evidence.digest(helper)
    producer_digest = evidence.digest(Path(__file__))
    revision, dirty = git_identity(source, env)
    before = snapshot(source)
    source_hashes = hashes(before)
    rust_command = [env.get("RUSTC", "rustc"), "-vV"]
    rust_info = inspect(rust_command, source, env)
    selected_target = target(source, env, rust_info)
    config_hashes = {str(path): evidence.digest(path) for path in configuration_files(source, env)}
    output.mkdir(parents=True, exist_ok=True)
    try:
        for name, data in before.items():
            path = output / "source" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        input_path = output / "census-input.tsv"
        input_path.write_text("dll\tinterface\tname\tnum_parameters\n" + "".join(
            "\t".join(row[field] for field in evidence.RESOLVER_COLUMNS[:4]) + "\n" for row in native), encoding="utf-8")
        input_digest = evidence.digest(input_path)
        env.update(RPORT_NATIVE_CENSUS_INPUT=str(input_path), RPORT_NATIVE_RESOLVER_OUTPUT=str(output / "resolver.tsv"))
        command = [str(wrapper), "test", "-p", "rmath", "--lib", "--message-format=json", EXPORTER,
                   "--", "--exact", "--ignored", "--nocapture", "--test-threads=1"]
        bounded = [sys.executable, str(helper), "--timeout", str(timeout), "--timeout-marker",
                   str(output / "timeout.marker"), "--", *command]
        with (output / "probe.log").open("wb") as stream:
            result = subprocess.run(bounded, cwd=source, env=env, stdout=stream, stderr=subprocess.STDOUT, check=False)
        require(not (output / "timeout.marker").exists(), "native exporter deadline expired")
        require(result.returncode == 0, f"native exporter exited {result.returncode}")
        log = (output / "probe.log").read_text(encoding="utf-8", errors="replace")
        require(len(FOOTER.findall(log)) == 1, "native exporter has no unique completed one-test footer")
        counts = COUNT.findall(log)
        require(counts == [str(len(native))], "native exporter row-count footer differs from census")
        artifact = compiler_artifact(log)
        executable = Path(artifact["executable"])
        if not executable.is_absolute():
            executable = source / executable
        binary_hash = evidence.digest(executable)
        resolver_rows = evidence.table(output / "resolver.tsv", evidence.RESOLVER_COLUMNS, "\t")
        evidence.reconcile(native, resolver_rows)
        require(hashes(snapshot(source)) == source_hashes, "compiled sources changed during native exporter build/probe")
        require(git_identity(source, env) == (revision, dirty), "source revision/dirty state changed during probe")
        require(evidence.digest(wrapper) == wrapper_digest, "Cargo wrapper changed during probe")
        require(evidence.digest(helper) == helper_digest and evidence.digest(Path(__file__)) == producer_digest,
                "inventory producer/deadline helper changed during probe")
        require(evidence.digest(input_path) == input_digest, "exporter census input changed during probe")
        require({str(path): evidence.digest(path) for path in configuration_files(source, env)} == config_hashes,
                "Cargo configuration changed during probe")
        require(inspect(rust_command, source, env) == rust_info, "Rust compiler selection changed during probe")
        require(evidence.digest(executable) == binary_hash, "compiled artifact changed before evidence publication")
        features = artifact.get("features")
        require(isinstance(features, list) and all(isinstance(feature, str) for feature in features), "Cargo artifact features are missing")
        flags = env.get("CARGO_ENCODED_RUSTFLAGS")
        effective_flags = flags.split("\x1f") if flags else shlex.split(env.get("RUSTFLAGS", ""))
        recorded_env = {name: value for name, value in sorted(env.items()) if name in {
            "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER",
            "RUSTUP_TOOLCHAIN", "CARGO_BUILD_TARGET", "CARGO_TARGET_DIR", "CARGO_HOME", "RPORT_NATIVE_CENSUS_INPUT",
            "RPORT_NATIVE_RESOLVER_OUTPUT"} or name.startswith("CARGO_PROFILE_")
            or name.startswith("CARGO_BUILD_") or name.startswith("CARGO_TARGET_")}
        provenance = {"schema_version": 1, "oracle_source_commit": manifest["source"]["commit"],
                      "oracle_manifest_sha256": evidence.manifest_digest(manifest_path),
                      "census_native_sha256": census_provenance["files_sha256"]["native_routines.csv"],
                      "census_provenance_sha256": evidence.digest(census / "provenance.json"),
                      "resolver_tsv_sha256": evidence.digest(output / "resolver.tsv"),
                      "source_revision": revision, "source_dirty": dirty, "source_files_sha256": source_hashes,
                      "compiled_artifact_sha256": binary_hash, "rust_version": rust_info,
                      "probe_command": bounded, "probe_exit_code": 0, "execution_complete": True,
                      "probe_log_sha256": evidence.digest(output / "probe.log"),
                      "build_profile": {"profile": "test", "target": selected_target, "flags": effective_flags,
                                        "default_features": "default" in features, "features": features,
                                        "compiler_profile": artifact["profile"], "environment": recorded_env,
                                        "compiled_artifact_path": str(executable), "working_directory": str(source),
                                        "cargo_wrapper_sha256": wrapper_digest, "configuration_files_sha256": config_hashes, "deadline_helper_sha256": helper_digest, "census_input_sha256": input_digest,
                                        "producer_script_sha256": producer_digest}}
        # Recheck census integrity after the long subprocess before associating
        # this completed probe with its immutable input provenance.
        evidence.checked_census(census, manifest_path)
        require((evidence.digest(census / "provenance.json"), evidence.manifest_digest(manifest_path)) == input_identity,
                "census/manifest identity changed during probe")
        temporary = output / "provenance.json.tmp"
        temporary.write_text(json.dumps(provenance, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        os.replace(temporary, output / "provenance.json")
        return provenance
    except BaseException as error:
        marker = output / "timeout.marker"
        failure = {"execution_complete": False, "error": str(error), "timeout": marker.exists()}
        (output / "failure.json").write_text(json.dumps(failure, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        if marker.exists():
            raise InventoryError("native exporter deadline expired; incomplete logs preserved", 124) from error
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("census", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--manifest", type=Path, default=evidence.DEFAULT_MANIFEST)
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--timeout", type=positive_seconds, default=600)
    args = parser.parse_args()
    try:
        provenance = generate(args.census, args.output, args.source_root, args.manifest, args.target_dir, args.timeout)
    except (InventoryError, evidence.EvidenceError, evidence.ManifestError, OSError, KeyError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return error.exit_code if isinstance(error, InventoryError) else 1
    print(f"Completed native resolver inventory at {args.output}: {len(provenance['source_files_sha256'])} source files retained. "
          "Registration evidence only; no handlers invoked.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
