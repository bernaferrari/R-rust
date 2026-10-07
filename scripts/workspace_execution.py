#!/usr/bin/env python3
"""Execute disjoint Cargo test targets from one unchanged workspace debug build.

Each producer builds the entire workspace, then runs only its admitted binaries.
Cargo's feature union is therefore identical to `cargo test --workspace`. The
final producer runs doctests and UniFFI. A union requires every target and phase,
authenticated process journals, matching source, toolchain and actual features.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

if __package__:
    from .upstream_execution import atomic_json, checked_process, digest, file_hash, run_process
else:
    from upstream_execution import atomic_json, checked_process, digest, file_hash, run_process

ROOT = Path(__file__).resolve().parents[1]
CARGO = str(ROOT / "scripts/cargo_dev.sh")
LIB_KINDS = {"lib", "rlib", "cdylib", "staticlib", "dylib"}
BUILD_ARGS = ["test", "--workspace", "--no-run", "--message-format=json"]


def source_identity(root):
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    names = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
    # Generated, untracked device output is not source. Tracked inputs, including
    # fixtures and PDFs, must remain byte-identical throughout execution.
    inputs = {name: file_hash(root / name) for name in names if name}
    return {"source_commit": commit, "source_sha256": digest(inputs)}


def inventory(metadata, root):
    root = root.resolve()
    rows = []
    members = set(metadata["workspace_members"])
    for package in metadata["packages"]:
        if package["id"] not in members:
            continue
        directory = Path(package["manifest_path"]).resolve().parent.relative_to(root).as_posix()
        for target in package["targets"]:
            kinds = target["kind"]
            if not (set(kinds) & (LIB_KINDS | {"bin", "test"})):
                continue
            source = Path(target["src_path"]).resolve().relative_to(root).as_posix()
            for phase, enabled in (("test", target["test"]), ("doc", target["doctest"] and bool(set(kinds) & LIB_KINDS))):
                if enabled:
                    if target.get("required-features"):
                        raise ValueError("required-feature target needs explicit admission: " + target["name"])
                    rows.append({"id": digest([package["name"], target["name"], kinds, phase]),
                                 "package": package["name"], "name": target["name"],
                                 "kind": kinds, "phase": phase, "source": source, "cwd": directory})
    rows.sort(key=lambda row: (row["phase"], row["package"], row["name"], row["kind"]))
    if not rows or len({row["id"] for row in rows}) != len(rows):
        raise ValueError("empty or duplicate workspace inventory")
    return rows


def partition(rows, index, count):
    tests = [row for row in rows if row["phase"] == "test"]
    if type(index) is not int or type(count) is not int or not 2 <= count <= len(tests) + 1 or not 0 <= index < count:
        raise ValueError("invalid workspace partition")
    if index == count - 1:
        return [row for row in rows if row["phase"] == "doc"]
    return tests[index::count - 1]


def cargo_artifacts(lines, metadata, root, *, require_files):
    packages = {p["id"]: p for p in metadata["packages"]}
    members = set(metadata["workspace_members"])
    selected, features, finished = {}, {}, []
    for number, line in enumerate(lines, 1):
        if not line.lstrip().startswith("{"):
            continue  # cargo_dev's pruning receipt follows Cargo's stream.
        try:
            message = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid Cargo JSON at line {number}") from error
        if message.get("reason") == "build-finished":
            finished.append(message.get("success"))
        if message.get("reason") != "compiler-artifact":
            continue
        package = packages[message["package_id"]]
        target, profile = message["target"], message["profile"]
        # Resolver v3 can build one dependency for both host/proc-macro and
        # runtime use, with different profiles or features. Preserve the full
        # emitted set instead of erasing or rejecting those valid variants.
        admission = {"package": package["name"], "version": package["version"],
                     "target": target["name"], "kind": target["kind"],
                     "features": sorted(message["features"]), "profile": profile}
        features[digest(admission)] = admission
        if message["package_id"] not in members or not profile["test"] or not message.get("executable"):
            continue
        if not profile["debug_assertions"] or profile["opt_level"] != "0":
            raise ValueError("workspace test profile is not the original debug profile")
        identity = digest([package["name"], target["name"], target["kind"], "test"])
        if identity in selected:
            raise ValueError("duplicate Cargo executable admission")
        path = Path(message["executable"])
        if require_files and not path.is_file():
            raise ValueError("emitted Cargo executable is missing")
        selected[identity] = str(path)
    if finished != [True]:
        raise ValueError("one successful Cargo build-finished receipt is required")
    expected = {row["id"] for row in inventory(metadata, root) if row["phase"] == "test"}
    if set(selected) != expected:
        raise ValueError("Cargo executable inventory differs from workspace metadata")
    return selected, features


def toolchain(root):
    return {"rustc": subprocess.check_output(["rustc", "-vV"], cwd=root, text=True),
            "cargo": subprocess.check_output([CARGO, "--version"], cwd=root, text=True),
            "build_environment": {name: os.environ.get(name) for name in (
                "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_BUILD_TARGET",
                "CARGO_PROFILE_TEST_OPT_LEVEL", "CARGO_PROFILE_TEST_DEBUG_ASSERTIONS")}}


def executable_environment(root, metadata, cargo_lines):
    # Cargo adds these directories when it launches test binaries. Preserve
    # inherited paths as well, and use the same target tmp directory.
    target = Path(metadata["target_directory"])
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], cwd=root, text=True).strip())
    environment = dict(os.environ)
    library_variable = "DYLD_FALLBACK_LIBRARY_PATH" if sys.platform == "darwin" else "LD_LIBRARY_PATH"
    target_library = subprocess.check_output(["rustc", "--print", "target-libdir"], cwd=root, text=True).strip()
    paths = [str(target / "debug/deps"), str(target / "debug"), target_library, str(sysroot / "lib")]
    for line in cargo_lines:
        if line.lstrip().startswith("{"):
            message = json.loads(line)
            if message.get("reason") == "build-script-executed":
                for linked in message.get("linked_paths", []):
                    path = Path(linked.split("=", 1)[-1]).resolve()
                    if path.is_relative_to(target.resolve()):
                        paths.append(str(path))
    if environment.get(library_variable):
        paths.append(environment[library_variable])
    environment[library_variable] = os.pathsep.join(paths)
    environment["CARGO_TARGET_TMPDIR"] = str(target / "tmp")
    (target / "tmp").mkdir(exist_ok=True)
    return environment


def produce(root, directory, index, count, timeout):
    root = root.resolve()
    directory.mkdir(parents=True)  # Refuse to replace earlier evidence.
    profile = {"schema_version": 1, **source_identity(root), **toolchain(root),
               "build": BUILD_ARGS, "timeout_seconds": timeout,
               "shard_index": index, "shard_count": count, "root": str(root)}
    atomic_json(directory / "profile.json", profile)
    report = {"profile": profile, "rows": [], "phases": {}, "errors": [],
              "execution_complete": False, "strict_pass": False}
    atomic_json(directory / "summary.json", report)
    deadline = time.monotonic() + timeout

    def execute(command, phase, *, cwd=root, combined=True, env=None):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ValueError("workspace group deadline exceeded; later targets remain unverified")
        return run_process(command, cwd=cwd, timeout=remaining, directory=directory / phase,
                           combined=combined, env=env)

    try:
        receipt = execute([CARGO, "metadata", "--format-version=1"], "metadata", combined=False)
        if receipt["exit_code"] or not receipt["execution_complete"]:
            raise ValueError("workspace metadata did not complete successfully")
        metadata = json.loads((directory / "metadata/stdout.log").read_text())
        rows = inventory(metadata, root)
        selected = partition(rows, index, count)
        atomic_json(directory / "inventory.json", rows)
        report["inventory_sha256"] = digest(rows)
        build_environment = {**os.environ, "RPORT_CARGO_ARTIFACTS_JSON": str(directory / "build/stdout.log")}
        receipt = execute([CARGO, *BUILD_ARGS], "build", combined=False, env=build_environment)
        if receipt["exit_code"] or not receipt["execution_complete"]:
            raise ValueError("workspace debug build did not complete successfully")
        cargo_lines = (directory / "build/stdout.log").read_text().splitlines()
        executables, features = cargo_artifacts(cargo_lines,
                                                metadata, root, require_files=True)
        report["actual_cargo_features"] = features
        environment = executable_environment(root, metadata, cargo_lines)
        if index != count - 1:
            for row in selected:
                executable = executables[row["id"]]
                phase = "target-" + row["id"]
                record = {**row, "executable": executable, "executable_sha256": file_hash(executable), "process": None}
                report["rows"].append(record)
                atomic_json(directory / "summary.json", report)
                record["process"] = execute([executable], phase, cwd=root / row["cwd"], env=environment)
                if file_hash(executable) != record["executable_sha256"]:
                    raise ValueError("test executable changed during execution")
                atomic_json(directory / "summary.json", report)
        else:
            for phase, command in (("doctests", [CARGO, "test", "--workspace", "--doc", "--no-fail-fast"]),
                                   ("uniffi", [str(root / "scripts/generate_uniffi_bindings.sh"), "--check"])):
                report["phases"][phase] = execute(command, phase)
                atomic_json(directory / "summary.json", report)
        if source_identity(root) != {key: profile[key] for key in ("source_commit", "source_sha256")}:
            raise ValueError("tracked workspace source changed during execution")
        report["execution_complete"] = all(p["execution_complete"] for p in (
            [row["process"] for row in report["rows"]] + list(report["phases"].values())))
        report["strict_pass"] = report["execution_complete"] and all(p["exit_code"] == 0 for p in (
            [row["process"] for row in report["rows"]] + list(report["phases"].values())))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        report["errors"].append(str(error))
    finally:
        atomic_json(directory / "summary.json", report)
    return report


def validate_producer(directory, rows, common, count):
    report = json.loads((directory / "summary.json").read_text())
    profile = report["profile"]
    index = profile["shard_index"]
    if type(index) is not int or profile != {**common, "shard_index": index, "root": profile["root"]}:
        raise ValueError("workspace execution policy/source differs")
    root = Path(profile["root"])
    selected = partition(rows, index, count)
    if report["errors"] or report["inventory_sha256"] != digest(rows):
        raise ValueError("producer error or changed inventory")
    if json.loads((directory / "inventory.json").read_text()) != rows:
        raise ValueError("producer inventory differs")
    metadata_receipt = checked_process(directory / "metadata")
    build_receipt = checked_process(directory / "build")
    for receipt, command in ((metadata_receipt, [str(root / "scripts/cargo_dev.sh"), "metadata", "--format-version=1"]),
                             (build_receipt, [str(root / "scripts/cargo_dev.sh"), *BUILD_ARGS])):
        if receipt["command"] != command or receipt["cwd"] != str(root) or not 0 < receipt["timeout_seconds"] <= common["timeout_seconds"] or not receipt["execution_complete"] or receipt["exit_code"]:
            raise ValueError("unsuccessful or mismatched Cargo phase")
    metadata = json.loads((directory / "metadata/stdout.log").read_text())
    if inventory(metadata, root) != rows:
        raise ValueError("Cargo metadata target inventory differs")
    executables, features = cargo_artifacts((directory / "build/stdout.log").read_text().splitlines(),
                                            metadata, root, require_files=False)
    if report["actual_cargo_features"] != features:
        raise ValueError("actual Cargo feature/profile receipt differs")
    receipts = []
    admitted = selected if index != count - 1 else []
    if [row["id"] for row in report["rows"]] != [row["id"] for row in admitted]:
        raise ValueError("missing, duplicate, or out-of-partition target")
    for expected, row in zip(admitted, report["rows"]):
        if any(row[key] != value for key, value in expected.items()):
            raise ValueError("test target identity differs")
        receipt = checked_process(directory / ("target-" + row["id"]), row["process"])
        if row["executable"] != executables[row["id"]] or receipt["command"] != [row["executable"]] or receipt["cwd"] != str(root / row["cwd"]):
            raise ValueError("test did not execute the admitted Cargo binary in its package directory")
        fingerprint = row["executable_sha256"]
        if not isinstance(fingerprint, str) or len(fingerprint) != 64 or any(c not in "0123456789abcdef" for c in fingerprint):
            raise ValueError("invalid test executable fingerprint")
        receipts.append(receipt)
    commands = {"doctests": [str(root / "scripts/cargo_dev.sh"), "test", "--workspace", "--doc", "--no-fail-fast"],
                "uniffi": [str(root / "scripts/generate_uniffi_bindings.sh"), "--check"]} if index == count - 1 else {}
    if set(report["phases"]) != set(commands):
        raise ValueError("missing or unexpected doctest/UniFFI phase")
    for phase, command in commands.items():
        receipt = checked_process(directory / phase, report["phases"][phase])
        if receipt["command"] != command or receipt["cwd"] != str(root):
            raise ValueError("doctest/UniFFI command differs")
        receipts.append(receipt)
    if any(not 0 < receipt["timeout_seconds"] <= common["timeout_seconds"] for receipt in receipts):
        raise ValueError("execution deadline differs")
    complete = all(receipt["execution_complete"] for receipt in receipts)
    passed = complete and all(receipt["exit_code"] == 0 for receipt in receipts)
    if report["execution_complete"] is not complete or report["strict_pass"] is not passed:
        raise ValueError("producer completion/pass claim differs from actual execution")
    return index, selected, features, complete, passed


def merge(root, directories, rows, count, commit, timeout):
    root = root.resolve()
    common = {"schema_version": 1, **source_identity(root), **toolchain(root), "build": BUILD_ARGS,
              "timeout_seconds": timeout, "shard_count": count}
    errors, seen, completed, passed, feature_union, admitted = [], set(), [], [], None, set()
    if common["source_commit"] != commit:
        errors.append("aggregation checkout differs from requested source commit")
    for directory in directories:
        try:
            index, selected, features, complete, success = validate_producer(directory, rows, common, count)
            if index in seen:
                raise ValueError("duplicate workspace shard")
            if feature_union is not None and feature_union != features:
                raise ValueError("workspace Cargo features/profile differ between producers")
            feature_union = features
            seen.add(index)
            admitted.update(row["id"] for row in selected)
            completed.append(complete)
            passed.append(success)
        except (OSError, ValueError, KeyError, TypeError) as error:
            errors.append(f"{directory.name}: {error}")
    missing = sorted(set(range(count)) - seen)
    if missing:
        errors.append(f"missing workspace shards: {missing}")
    unverified = [row for row in rows if row["id"] not in admitted]
    complete = not errors and not unverified and all(completed)
    return {"profile": common, "inventory": rows, "inventory_total": len(rows),
            "unverified_targets": unverified, "errors": errors,
            "execution_complete": complete, "strict_pass": complete and all(passed),
            "scope": "All default workspace test executables, doctests and generated UniFFI bindings; Cargo's existing ignored tests stay ignored."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("produce", "merge"))
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--shard-index", type=int)
    parser.add_argument("--shard-count", type=int, default=9)
    parser.add_argument("--source-commit")
    parser.add_argument("--timeout", type=float, default=1800)
    args = parser.parse_args()
    root = args.root.resolve()
    if args.mode == "produce":
        report = produce(root, args.directory.resolve(), args.shard_index, args.shard_count, args.timeout)
    else:
        # Independent metadata admission does not compile or execute the runtime.
        metadata = json.loads(subprocess.check_output([CARGO, "metadata", "--format-version=1", "--no-deps"], cwd=root, text=True))
        rows = inventory(metadata, root)
        directories = sorted(path.parent for path in args.directory.glob("*/summary.json"))
        report = merge(root, directories, rows, args.shard_count, args.source_commit, args.timeout)
        args.directory.mkdir(parents=True, exist_ok=True)
        atomic_json(args.directory / "summary.json", report)
    print(json.dumps({key: report[key] for key in ("execution_complete", "strict_pass", "errors")}, indent=2))
    return 0 if report["strict_pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
