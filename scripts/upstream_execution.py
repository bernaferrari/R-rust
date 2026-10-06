#!/usr/bin/env python3
"""Durable bounded execution and exact inventory joins for pinned upstream tests.

Hashes bind evidence to inputs; they are integrity checks, not an attestation of
arbitrary hand-written JSON. Engine completion and output parity are separate.
"""
from __future__ import annotations

import argparse
from contextlib import ExitStack
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import signal
import shlex
import platform
import shutil
import subprocess
import sys
import tempfile
import time

# Direct invocation and unittest package import use the same implementations.
if __package__:
    from .run_parity_case import positive_seconds, stop_group
    from .validate_r_oracle import load_manifest
    from .validate_upstream_r_tests import validate_corpus
else:
    from run_parity_case import positive_seconds, stop_group
    from validate_r_oracle import load_manifest
    from validate_upstream_r_tests import validate_corpus

ROOT = Path(__file__).resolve().parent.parent
STATUSES = ("pass", "fail", "xfail", "xpass", "skip")
NORMALIZATION = "core-v1:original-tr-sed-awk-pipeline"
CASE_LOCALES = {"whole/reg-plot-latin1.R": "en_US.ISO8859-1"}


def runtime_configuration(runtime_profile, package_policy):
    if runtime_profile not in {"core", "graphics"} or package_policy not in {"native", "portable"}:
        raise ValueError("invalid runtime profile or package policy")
    features = ["default", "faer", "rust-backend"]
    device = None
    if runtime_profile == "graphics":
        features += ["r-graphics-engine", "renderplot-device"]
        device = {"name": "renderplot-scene", "width": 504, "height": 504,
                  "font": "bundled DejaVu Sans", "output": "owned display list"}
    return {"name": runtime_profile, "cargo_features": features,
            "device": device, "package_policy": package_policy, "numerical_backend": "faer",
            "top_level_evaluation_mode": "Script",
            "gnu_device_policy": "Rscript default device; driver device calls unchanged"}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def file_hash(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def stamp():
    return datetime.now(timezone.utc).isoformat()


def atomic_json(path, value):
    path = Path(path)
    descriptor, name = tempfile.mkstemp(prefix=path.name + ".", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def append_event(path, value):
    with Path(path).open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, sort_keys=True) + "\n")
        stream.flush()
        os.fsync(stream.fileno())


class Cancelled(BaseException):
    def __init__(self, number):
        self.number = number


def run_process(command, *, cwd, timeout, directory, combined=True, env=None):
    """Own exactly one new POSIX process group; publish START before invocation."""
    try:
        timeout = positive_seconds(str(timeout))
    except argparse.ArgumentTypeError as error:
        raise ValueError(str(error)) from error
    if os.name != "posix" or not command:
        raise ValueError("a command and POSIX process groups are required")
    directory = Path(directory)
    directory.mkdir()  # A phase is never reused or silently overwritten.
    receipt = {"schema_version": 1, "command": [str(part) for part in command],
               "cwd": str(Path(cwd).resolve()), "timeout_seconds": timeout,
               "started_at": stamp(), "state": "started", "execution_complete": False}
    atomic_json(directory / "process.json", receipt)
    append_event(directory / "events.jsonl", {"event": "START", **receipt})
    print(f"START {directory.name}: {Path(command[0]).name}", flush=True)
    started, process = time.monotonic(), None
    handlers = {number: signal.getsignal(number) for number in (signal.SIGTERM, signal.SIGINT)}
    def cancel(number, _frame):
        raise Cancelled(number)
    for number in handlers:
        signal.signal(number, cancel)
    output_paths = [directory / "combined.log"] if combined else [directory / "stdout.log", directory / "stderr.log"]
    try:
        with ExitStack() as streams:
            stdout = streams.enter_context(output_paths[0].open("xb"))
            stderr = subprocess.STDOUT if combined else streams.enter_context(output_paths[1].open("xb"))
            try:
                # Block cancellation through fork/assignment so cleanup always
                # has the actual owned child, even if TERM arrives at launch.
                mask = signal.pthread_sigmask(signal.SIG_BLOCK, set(handlers))
                try:
                    process = subprocess.Popen(command, cwd=cwd, env=env, stdout=stdout, stderr=stderr,
                                               start_new_session=True)
                finally:
                    signal.pthread_sigmask(signal.SIG_SETMASK, mask)
                receipt["pid"] = process.pid
                atomic_json(directory / "process.json", receipt)
                code = process.wait(timeout=timeout)
                # Do not permit an orphan to continue writing after FINISH.
                stop_group(process)
                receipt.update(state="finished", exit_code=code, execution_complete=True)
            except subprocess.TimeoutExpired:
                stop_group(process)
                receipt.update(state="timeout", exit_code=124, execution_complete=False)
            except Cancelled as error:
                for number in handlers:
                    signal.signal(number, signal.SIG_IGN)
                if process is not None:
                    stop_group(process)
                receipt.update(state="cancelled", exit_code=128 + error.number, execution_complete=False)
            except OSError as error:
                if process is not None:
                    stop_group(process)
                receipt.update(state="launch_error", exit_code=2, execution_complete=False, error=str(error))
            except BaseException:
                for number in handlers:
                    signal.signal(number, signal.SIG_IGN)
                if process is not None:
                    stop_group(process)
                receipt.update(state="interrupted", exit_code=2, execution_complete=False)
                raise
    finally:
        for number, handler in handlers.items():
            signal.signal(number, handler)
        receipt.update(finished_at=stamp(), elapsed_seconds=time.monotonic() - started,
                       logs={path.name: file_hash(path) for path in output_paths if path.is_file()})
        atomic_json(directory / "process.json", receipt)
        append_event(directory / "events.jsonl", {"event": "FINISH", **receipt})
        print(f"FINISH {directory.name}: {receipt['state']} exit={receipt.get('exit_code')}", flush=True)
    return receipt


def normalizer_policy():
    tools = {}
    for name in ("bash", "tr", "sed", "awk"):
        executable = shutil.which(name)
        if executable is None:
            raise ValueError(f"missing original normalization tool {name}")
        path = Path(executable).resolve()
        tools[name] = {"path": str(path), "sha256": file_hash(path)}
    return {"platform": {"system": platform.system(), "machine": platform.machine()},
            "locale": {key: "C" for key in ("LANG", "LC_ALL", "LC_CTYPE")},
            "tools": tools}


def normalizer_command(policy, source):
    tools = {name: shlex.quote(row["path"]) for name, row in policy["tools"].items()}
    # Exact original commands and awk program. Absolute tool paths authenticate
    # the executables selected by the original caller's PATH.
    program = (tools["tr"] + r" -d '\r' " + '<"$1" | ' + tools["sed"]
               + " 's/[[:space:]]*$//' | " + tools["awk"]
               + " '/^Time elapsed:/ { next } { lines[++n] = $0 } END { while (n > 0 && lines[n] == \"\") n--; for (i = 1; i <= n; i++) print lines[i] }'")
    return [policy["tools"]["bash"]["path"], "-o", "pipefail", "-c", program, "_", str(source)]


def run_normalization(directory, engine, policy, timeout):
    directory = Path(directory).resolve()
    source = directory / engine / "combined.log"
    environment = dict(os.environ)
    for key, value in policy["locale"].items():
        if value is None:
            environment.pop(key, None)
        else:
            environment[key] = value
    receipt = run_process(normalizer_command(policy, source), cwd=directory, timeout=timeout,
                          directory=directory / (engine + "-normalize"), combined=False, env=environment)
    return {"engine": engine, "input_sha256": file_hash(source), "process": receipt}


def catalog(root):
    root = Path(root)
    oracle = load_manifest(root / "oracle/r-oracle.json")
    corpus = root / "tests/upstream-r"
    report = validate_corpus(corpus, oracle["source"]["commit"])
    curated = sorted((root / "tests/upstream-core/cases").glob("*.R"))
    if not curated:
        raise ValueError("empty curated upstream inventory")
    xfails = set()
    for line in (root / "tests/upstream-core/xfail.tsv").read_text().splitlines():
        if line and not line.startswith("#"):
            name = line.split("\t")[0]
            if name in xfails:
                raise ValueError("duplicate curated xfail")
            xfails.add(name)
    names = {path.stem for path in curated}
    if not xfails <= names:
        raise ValueError("curated xfail not in inventory")
    rows = [{"kind": "curated", "case": path.name,
             "path": path.relative_to(root).as_posix(),
             "disposition": "xfail" if path.stem in xfails else "pass", "owner": "-", "reason": "-"}
            for path in curated]
    rows.extend({"kind": "whole", "case": entry.path,
                 "path": "tests/upstream-r/vendor/" + entry.path,
                 "disposition": entry.disposition, "owner": entry.owner, "reason": entry.reason}
                for entry in report.entries)
    return rows


def partition(rows, suite, index, count):
    if suite not in {"all", "curated", "whole"}:
        raise ValueError("invalid upstream suite")
    selected = [row for row in rows if suite == "all" or row["kind"] == suite]
    if type(count) is not int or type(index) is not int or not 1 <= count <= len(selected) or not 0 <= index < count:
        raise ValueError("shard index/count must select a nonempty partition")
    return selected[index::count]


def source_inputs(root):
    paths = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root).decode().split("\0")
    return {path: file_hash(Path(root) / path) for path in sorted(set(paths)) if path and (
        path.startswith(("crates/", "vendor/", ".cargo/", "tests/conformance/src/", "tests/upstream-core/", "tests/upstream-r/"))
        or path in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".github/workflows/ci.yml", "oracle/r-oracle.json"}
        or path.startswith("scripts/") and not path.endswith(".pyc")
    ) and (Path(root) / path).is_file()}


def contract_at(root, *, suite, index, count, timeout, profile, rustflags, strict, pinned, normalizer=None,
                runtime_profile="core", package_policy="native"):
    try:
        timeout = positive_seconds(str(timeout))
    except argparse.ArgumentTypeError as error:
        raise ValueError(str(error)) from error
    if profile not in {"debug", "release"}:
        raise ValueError("invalid profile")
    rows = catalog(root)
    selected = partition(rows, suite, index, count)
    inputs = source_inputs(root)
    return {"schema_version": 1,
            "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
            "source_files_sha256": inputs, "source_sha256": digest(inputs),
            "inventory": rows, "inventory_sha256": digest(rows),
            "oracle_manifest_sha256": file_hash(Path(root) / "oracle/r-oracle.json"),
            "profile": profile, "rustflags": rustflags, "case_timeout_seconds": timeout,
            "runtime": runtime_configuration(runtime_profile, package_policy),
            "case_locale_policy": {"default": "C", "overrides": CASE_LOCALES},
            "strict": strict, "pinned_oracle_required": pinned, "normalization": NORMALIZATION,
            "normalizer_policy": normalizer_policy() if normalizer is None else normalizer,
            "shard": {"suite": suite, "index": index, "count": count,
                      "selected_sha256": digest(selected), "selected_total": len(selected)}}


def fresh_directory(path):
    path = Path(path)
    if path.exists() and (not path.is_dir() or any(path.iterdir())):
        raise ValueError("report directory must be new or empty")
    path.mkdir(parents=True, exist_ok=True)


def verify_normalizer_policy(policy):
    if not isinstance(policy, dict) or set(policy) != {"platform", "locale", "tools"}:
        raise ValueError("invalid normalization producer policy")
    if set(policy["platform"]) != {"system", "machine"} or any(not isinstance(value, str) or not value for value in policy["platform"].values()):
        raise ValueError("invalid normalization producer platform")
    if set(policy["locale"]) != {"LANG", "LC_ALL", "LC_CTYPE"} or any(value is not None and not isinstance(value, str) for value in policy["locale"].values()):
        raise ValueError("invalid normalization producer locale")
    if policy["locale"] != {key: "C" for key in ("LANG", "LC_ALL", "LC_CTYPE")}:
        raise ValueError("normalization requires the byte-preserving C locale")
    if set(policy["tools"]) != {"bash", "tr", "sed", "awk"}:
        raise ValueError("missing or extra original normalization tool")
    for row in policy["tools"].values():
        if set(row) != {"path", "sha256"} or not isinstance(row["path"], str) or not Path(row["path"]).is_absolute():
            raise ValueError("invalid normalization tool path")
        value = row["sha256"]
        if not isinstance(value, str) or len(value) != 64 or any(character not in "0123456789abcdef" for character in value):
            raise ValueError("invalid normalization tool hash")


def verify_contract(root, contract, *, producer_environment=True):
    if type(contract["schema_version"]) is not int or contract["schema_version"] != 1:
        raise ValueError("invalid contract schema")
    if any(type(contract[key]) is not bool for key in ("strict", "pinned_oracle_required")):
        raise ValueError("invalid boolean execution policy")
    if type(contract["case_timeout_seconds"]) not in (int, float):
        raise ValueError("invalid process deadline type")
    verify_normalizer_policy(contract["normalizer_policy"])
    shard = contract["shard"]
    if type(shard["selected_total"]) is not int:
        raise ValueError("invalid selected inventory count")
    actual = contract_at(root, suite=shard["suite"], index=shard["index"], count=shard["count"],
                         timeout=contract["case_timeout_seconds"], profile=contract["profile"],
                         rustflags=contract["rustflags"], strict=contract["strict"], pinned=contract["pinned_oracle_required"],
                         runtime_profile=contract["runtime"]["name"], package_policy=contract["runtime"]["package_policy"],
                         normalizer=None if producer_environment else contract["normalizer_policy"])
    if actual != contract:
        raise ValueError("compiled source, fixtures, dispositions, oracle or policy changed")


def verdict(row, phases, directory, normalizations=()):
    if row["disposition"] == "skip":
        if phases or normalizations:
            raise ValueError("skipped driver has engine phases")
        return "skip", "declared skip"
    if len(phases) == 1 and phases[0]["engine"] == "gnu" and phases[0]["process"]["state"] != "finished":
        return "fail", "incomplete GNU process"
    if not phases or phases[0]["engine"] != "gnu":
        raise ValueError("missing GNU phase")
    stock = phases[0]["process"]
    if stock["state"] != "finished" or stock["exit_code"] != 0:
        if len(phases) != 1:
            raise ValueError("Rust was run after unsuccessful GNU admission")
        detail = "stock R exited non-zero" if stock["state"] == "finished" else "incomplete GNU process"
        same = False
    else:
        if len(phases) != 2 or phases[1]["engine"] != "rust":
            raise ValueError("missing Rust phase after successful GNU")
        rust = phases[1]["process"]
        if rust["state"] != "finished" or rust["exit_code"] != 0:
            detail = "Rust runner exited non-zero" if rust["state"] == "finished" else "incomplete Rust process"
            same = False
        else:
            if [entry["engine"] for entry in normalizations] != ["gnu", "rust"]:
                raise ValueError("missing original normalization phases")
            if any(not entry["process"]["execution_complete"] or entry["process"]["exit_code"] != 0 for entry in normalizations):
                return "fail", "normalization harness failure"
            same = (directory / "gnu-normalize/stdout.log").read_bytes() == (directory / "rust-normalize/stdout.log").read_bytes()
            detail = "" if same else "Rust output diverged from stock R"
    # A timeout or cancellation is never an expected semantic failure.
    if any(not phase["process"]["execution_complete"] for phase in phases):
        return "fail", detail
    return ("xpass" if same else "xfail", detail) if row["disposition"] == "xfail" else ("pass" if same else "fail", detail)


def summarize(contract, rows, *, error=None, finalized=False):
    selected = partition(
        contract["inventory"], contract["shard"]["suite"], contract["shard"]["index"], contract["shard"]["count"])
    keys = [(row["kind"], row["case"]) for row in rows]
    expected = {(row["kind"], row["case"]) for row in selected}
    complete = finalized is True and len(keys) == len(set(keys)) and set(keys) == expected and not error and all(
        all(phase["process"]["execution_complete"] for phase in row["phases"])
        and all(phase["process"]["execution_complete"] and phase["process"]["exit_code"] == 0
                for phase in row["normalizations"]) for row in rows)
    counts = {status: sum(row["status"] == status for row in rows) for status in STATUSES}
    return {"schema_version": 1, "execution": contract, "execution_finalized": finalized,
            "execution_complete": complete,
            "strict_pass": bool(complete and not counts["fail"] and not counts["xpass"]),
            "inventory_total": len(selected), "total": len(rows), "status_counts": counts,
            "error": error, "cases": rows,
            "unattempted_cases": [row for row in selected if (row["kind"], row["case"]) not in keys]}


def publish_report(directory, contract, rows, error=None, finalized=False):
    report = summarize(contract, rows, error=error, finalized=finalized)
    atomic_json(directory / "summary.json", report)
    lines = ["# Pinned GNU R upstream execution", "",
             f"Execution complete: **{report['execution_complete']}**",
             f"Strict parity passed: **{report['strict_pass']}**", "",
             f"Runtime: **{contract['runtime']['name']}**; features: `{','.join(contract['runtime']['cargo_features'])}`",
             f"Device: `{json.dumps(contract['runtime']['device'], sort_keys=True)}`",
             f"Package policy: **{contract['runtime']['package_policy']}**; numerical backend: **{contract['runtime']['numerical_backend']}**", "",
             f"Top-level evaluation: **{contract['runtime']['top_level_evaluation_mode']}** (preserve user `.Last.value`)", "",
             "| Case | Status | Detail |", "| --- | --- | --- |"]
    lines.extend(f"| {row['kind']}/{row['case']} | {row['status']} | {row['detail']} |" for row in rows)
    (directory / "summary.md").write_text("\n".join(lines) + "\n")
    return report


def execute(root, directory, contract, gnu, rust):
    root, directory = Path(root), Path(directory)
    gnu, rust = Path(gnu).resolve(), Path(rust).resolve()
    verify_contract(root, contract)  # Compiled-source admission before any case.
    rows, error = [], None
    selected = partition(contract["inventory"], contract["shard"]["suite"], contract["shard"]["index"], contract["shard"]["count"])
    publish_report(directory, contract, rows)
    for entry in selected:
        case_dir = directory / "cases" / (entry["kind"] + "-" + entry["case"])
        case_dir.mkdir(parents=True)
        phases, normalizations = [], []
        try:
            if entry["disposition"] != "skip":
                for engine, command in (("gnu", [str(gnu), "--vanilla"]), ("rust", [str(rust)])):
                    workspace = case_dir / (engine + "-workspace")
                    shutil.copytree((root / entry["path"]).parent, workspace)
                    locale = contract["case_locale_policy"]["overrides"].get(
                        entry["kind"] + "/" + entry["case"], contract["case_locale_policy"]["default"])
                    process = run_process(command + [entry["case"]], cwd=workspace,
                        timeout=contract["case_timeout_seconds"], directory=case_dir / engine,
                        env={**os.environ, "LC_ALL": locale, "LANG": locale, "TZ": "UTC", "SRCDIR": str(workspace),
                             "RPORT_RUNTIME_PACKAGE_POLICY": contract["runtime"]["package_policy"],
                             "RPORT_RUNTIME_RECEIPT": str((case_dir / "runtime-info.txt").resolve())})
                    phases.append({"engine": engine, "process": process})
                    shutil.rmtree(workspace)
                    if not process["execution_complete"] or process["exit_code"] != 0:
                        break
            if len(phases) == 2 and all(phase["process"]["execution_complete"] and phase["process"]["exit_code"] == 0 for phase in phases):
                for engine in ("gnu", "rust"):
                    normalizations.append(run_normalization(case_dir, engine, contract["normalizer_policy"], contract["case_timeout_seconds"]))
            status, detail = verdict(entry, phases, case_dir, normalizations)
            runtime_info = case_dir / "runtime-info.txt"
            rows.append({**entry, "status": status, "detail": detail, "phases": phases, "normalizations": normalizations,
                         "runtime_info_sha256": file_hash(runtime_info) if runtime_info.exists() else None})
            print(f"{status.upper()} {entry['kind']}/{entry['case']}: {detail}", flush=True)
            report = publish_report(directory, contract, rows)
            if any(phase["process"]["state"] == "cancelled" for phase in phases + normalizations):
                error = "execution cancelled"
                break
        except BaseException as caught:
            error = f"case execution interrupted: {type(caught).__name__}: {caught}"
            publish_report(directory, contract, rows, error)
            raise
    try:
        verify_contract(root, contract)
        artifacts = json.loads((directory / "artifacts.json").read_text())
        if any(file_hash(path) != artifacts[engine]["sha256"] for engine, path in (("gnu", gnu), ("rust", rust))):
            raise ValueError("executed engine artifact changed")
    except (ValueError, OSError, subprocess.CalledProcessError) as caught:
        error = str(caught)
    return publish_report(directory, contract, rows, error, finalized=error is None)


def seal_library(root, directory, rlib):
    contract = json.loads((directory / "contract.json").read_text())
    verify_contract(root, contract)
    receipt = checked_process(directory / "library-build")
    if not receipt["execution_complete"] or receipt["exit_code"] != 0:
        raise ValueError("library build did not complete successfully")
    selected = Path((directory / "library-build/stdout.log").read_text().strip()).resolve()
    if selected != Path(rlib).resolve():
        raise ValueError("library differs from actual Cargo artifact selection")
    sealed = {"original": str(selected), "sha256": file_hash(selected),
              "compiled_source_sha256": contract["source_sha256"]}
    cargo = directory / "library-cargo.json"
    if cargo.exists():
        sealed["cargo_sha256"] = file_hash(cargo)
    elif contract["runtime"]["name"] == "graphics":
        raise ValueError("graphics build has no Cargo feature/device receipt")
    atomic_json(directory / "library.json", sealed)


def sealed_inputs(directory, original, sha256, contract):
    sealed = {"original": original, "sha256": sha256,
              "compiled_source_sha256": contract["source_sha256"]}
    cargo = directory / "library-cargo.json"
    if cargo.exists():
        sealed["cargo_sha256"] = file_hash(cargo)
    elif contract["runtime"]["name"] == "graphics":
        raise ValueError("graphics build has no Cargo feature/device receipt")
    return sealed


def capture_artifacts(directory, gnu, rust, rlib):
    artifacts = {"rust_rlib_sha256": file_hash(rlib), "rust_rlib_original": str(Path(rlib).resolve())}
    sealed = json.loads((directory / "library.json").read_text())
    contract = json.loads((directory / "contract.json").read_text())
    if sealed != sealed_inputs(directory, artifacts["rust_rlib_original"], artifacts["rust_rlib_sha256"], contract):
        raise ValueError("library or compiled source changed during runner compilation")
    engines = directory / "engines"
    engines.mkdir()
    for name, original in (("gnu", gnu), ("rust", rust)):
        original = Path(original).resolve()
        copied = engines / name
        shutil.copyfile(original, copied)
        artifacts[name] = {"original": str(original), "sha256": file_hash(copied),
                           "file": "engines/" + name}
        if file_hash(original) != artifacts[name]["sha256"]:
            raise ValueError("engine changed during snapshot")
    tools = directory / "normalizer-tools"
    tools.mkdir()
    for name, row in contract["normalizer_policy"]["tools"].items():
        shutil.copyfile(row["path"], tools / name)
        if file_hash(tools / name) != row["sha256"]:
            raise ValueError("normalization tool changed during snapshot")
    atomic_json(directory / "artifacts.json", artifacts)
    return artifacts


def checked_process(location, expected=None):
    receipt = json.loads((location / "process.json").read_text())
    if type(receipt["schema_version"]) is not int or receipt["schema_version"] != 1:
        raise ValueError("invalid process receipt schema")
    events = [json.loads(line) for line in (location / "events.jsonl").read_text().splitlines()]
    if expected is not None and receipt != expected:
        raise ValueError("engine receipt differs from case result")
    if len(events) != 2 or events[0].get("event") != "START" or events[1] != {"event": "FINISH", **receipt}:
        raise ValueError("engine receipt/journal missing, incomplete or changed")
    if receipt["state"] not in {"finished", "timeout", "cancelled", "launch_error", "interrupted"}:
        raise ValueError("unknown engine process state")
    if type(receipt["execution_complete"]) is not bool or receipt["execution_complete"] != (receipt["state"] == "finished"):
        raise ValueError("inconsistent engine completion")
    if type(receipt["exit_code"]) is not int or set(receipt["logs"]) not in ({"combined.log"}, {"stdout.log", "stderr.log"}):
        raise ValueError("invalid process logs")
    if any(file_hash(location / name) != value for name, value in receipt["logs"].items()):
        raise ValueError("engine output changed")
    start = {key: receipt[key] for key in ("schema_version", "command", "cwd", "timeout_seconds", "started_at")}
    if events[0] != {"event": "START", **start, "state": "started", "execution_complete": False}:
        raise ValueError("engine START differs from FINISH")
    if type(receipt["elapsed_seconds"]) not in (int, float) or not 0 <= receipt["elapsed_seconds"] < float("inf"):
        raise ValueError("invalid process duration")
    try:
        positive_seconds(str(receipt["timeout_seconds"]))
    except argparse.ArgumentTypeError as error:
        raise ValueError(str(error)) from error
    return receipt


def checked_phase(directory, phase):
    engine = phase["engine"]
    if engine not in {"gnu", "rust"}:
        raise ValueError("unknown engine")
    receipt = checked_process(directory / engine, phase["process"])
    if set(receipt["logs"]) != {"combined.log"}:
        raise ValueError("core comparison needs exact combined engine logs")


def checked_builds(directory, contract):
    phases = ["library-build", "runner-build"]
    if contract["pinned_oracle_required"]:
        phases.append("oracle-validation")
    for name in phases:
        receipt = checked_process(directory / name)
        if not receipt["execution_complete"] or receipt["exit_code"] != 0:
            raise ValueError("incomplete or unsuccessful build/oracle admission")
        if receipt["timeout_seconds"] != contract["case_timeout_seconds"]:
            raise ValueError("build/oracle deadline differs from contract")
    cargo = directory / "library-cargo.json"
    if not cargo.exists():
        if contract["runtime"]["name"] == "graphics":
            raise ValueError("graphics build has no Cargo feature/device receipt")
        return
    messages = [json.loads(line) for line in cargo.read_text().splitlines() if line.lstrip().startswith("{")]
    libraries = [message for message in messages if message.get("reason") == "compiler-artifact"
                 and message.get("target", {}).get("name") == "rmath"
                 and not message.get("profile", {}).get("test")]
    if len(libraries) != 1 or sorted(libraries[0].get("features", [])) != sorted(contract["runtime"]["cargo_features"]):
        raise ValueError("actual Cargo features differ from runtime profile")
    selected = (directory / "library-build/stdout.log").read_text().strip()
    if selected not in libraries[0].get("filenames", []):
        raise ValueError("selected library differs from Cargo feature receipt")
    command = checked_process(directory / "runner-build")["command"]
    if "rmath=" + selected not in command:
        raise ValueError("runner did not link the selected runtime library")
    installed = "rport_renderplot" in command
    if installed != (contract["runtime"]["name"] == "graphics"):
        raise ValueError("runner device admission differs from runtime profile")
    if installed:
        graphics = {filename for message in messages if message.get("reason") == "compiler-artifact"
                    and message.get("target", {}).get("name") == "r_graphics_engine"
                    for filename in message.get("filenames", []) if filename.endswith(".rlib")}
        if len(graphics) != 1 or "r_graphics_engine=" + next(iter(graphics)) not in command:
            raise ValueError("runner did not link the emitted graphics engine")


def validate_report(root, directory, expected_common):
    report = json.loads((directory / "summary.json").read_text())
    if type(report["schema_version"]) is not int or report["schema_version"] != 1:
        raise ValueError("invalid report schema")
    if any(type(report[key]) is not int or report[key] < 0 for key in ("total", "inventory_total")):
        raise ValueError("invalid report counts")
    if any(type(value) is not int or value < 0 for value in report["status_counts"].values()):
        raise ValueError("invalid status counts")
    if any(type(report[key]) is not bool for key in ("execution_complete", "strict_pass")):
        raise ValueError("invalid report completion/parity type")
    contract = json.loads((directory / "contract.json").read_text())
    if report["execution"] != contract or {key: value for key, value in contract.items() if key not in {"shard", "normalizer_policy"}} != expected_common:
        raise ValueError("source, corpus, oracle, build profile or policy differs")
    verify_contract(root, contract, producer_environment=False)
    for name, row in contract["normalizer_policy"]["tools"].items():
        if file_hash(directory / "normalizer-tools" / name) != row["sha256"]:
            raise ValueError("archived normalization tool differs from producer policy")
    shard = contract["shard"]
    selected = partition(contract["inventory"], shard["suite"], shard["index"], shard["count"])
    if shard["selected_total"] != len(selected) or shard["selected_sha256"] != digest(selected):
        raise ValueError("invalid shard partition")
    checked_builds(directory, contract)
    artifacts = json.loads((directory / "artifacts.json").read_text())
    rlib_hash = artifacts["rust_rlib_sha256"]
    if not isinstance(rlib_hash, str) or len(rlib_hash) != 64 or any(c not in "0123456789abcdef" for c in rlib_hash):
        raise ValueError("missing compiled library hash")
    # These are producer-side paths sealed during compilation. Resolving them
    # on the consumer can follow unrelated mount aliases (macOS /home, for
    # example) and change the identity of an otherwise exact Cargo receipt.
    if (directory / "library-build/stdout.log").read_text().strip() != artifacts["rust_rlib_original"]:
        raise ValueError("selected library differs from actual Cargo artifact receipt")
    sealed = json.loads((directory / "library.json").read_text())
    if sealed != sealed_inputs(directory, artifacts["rust_rlib_original"], rlib_hash, contract):
        raise ValueError("compiled source/library seal differs")
    for engine in ("gnu", "rust"):
        if artifacts[engine]["file"] != "engines/" + engine or file_hash(directory / artifacts[engine]["file"]) != artifacts[engine]["sha256"]:
            raise ValueError("engine artifact changed")
    allowed = {(row["kind"], row["case"]): row for row in selected}
    seen = set()
    for row in report["cases"]:
        key = (row["kind"], row["case"])
        if key not in allowed or key in seen or any(row[field] != value for field, value in allowed[key].items()):
            raise ValueError("duplicate, extra or altered case disposition")
        seen.add(key)
        location = directory / "cases" / (row["kind"] + "-" + row["case"])
        runtime_info = location / "runtime-info.txt"
        if row.get("runtime_info_sha256") != (file_hash(runtime_info) if runtime_info.exists() else None):
            raise ValueError("initialized runtime policy receipt changed")
        phases = row["phases"]
        if len(phases) > 2 or len({phase["engine"] for phase in phases}) != len(phases):
            raise ValueError("duplicate or extra engine phase")
        for phase in phases:
            checked_phase(location, phase)
            process, engine = phase["process"], phase["engine"]
            command = [artifacts[engine]["original"]] + (["--vanilla"] if engine == "gnu" else []) + [row["case"]]
            if process["command"] != command or process["timeout_seconds"] != contract["case_timeout_seconds"]:
                raise ValueError("engine command or deadline differs")
        normalizations = row["normalizations"]
        if normalizations and not (len(phases) == 2 and all(phase["process"]["execution_complete"] and phase["process"]["exit_code"] == 0 for phase in phases)):
            raise ValueError("normalization run before successful engine completion")
        if normalizations and [phase["engine"] for phase in normalizations] != ["gnu", "rust"]:
            raise ValueError("duplicate or missing normalization phase")
        for normalization in normalizations:
            engine = normalization["engine"]
            receipt = checked_process(location / (engine + "-normalize"), normalization["process"])
            original_directory = Path(phases[0]["process"]["cwd"]).parent
            expected = normalizer_command(contract["normalizer_policy"], original_directory / engine / "combined.log")
            if (receipt["command"] != expected or receipt["timeout_seconds"] != contract["case_timeout_seconds"]
                    or normalization["input_sha256"] != file_hash(location / engine / "combined.log")
                    or set(receipt["logs"]) != {"stdout.log", "stderr.log"}):
                raise ValueError("normalization command, input or deadline differs")
        status, detail = verdict(allowed[key], phases, location, normalizations)
        if row["status"] != status or row["detail"] != detail:
            raise ValueError("declared result differs from actual engine output/exit")
    if type(report["execution_finalized"]) is not bool:
        raise ValueError("invalid finalization claim")
    recomputed = summarize(contract, report["cases"], error=report["error"], finalized=report["execution_finalized"])
    if report != recomputed:
        raise ValueError("case counts/completion/parity claim differs from evidence")
    return report


def merge(root, directories, *, count, commit, timeout, profile, rustflags,
          runtime_profile="core", package_policy="native"):
    expected = contract_at(root, suite="all", index=0, count=1, timeout=timeout,
                           profile=profile, rustflags=rustflags, strict=True, pinned=True, normalizer={},
                           runtime_profile=runtime_profile, package_policy=package_policy)
    common = {key: value for key, value in expected.items() if key not in {"shard", "normalizer_policy"}}
    partition(expected["inventory"], "whole", 0, count)
    rows, errors, seen, producer_policy = [], [], set(), None
    if expected["source_commit"] != commit:
        errors.append("aggregation checkout does not match requested commit")
    for directory in directories:
        try:
            report = validate_report(root, Path(directory), common)
            policy = report["execution"]["normalizer_policy"]
            if producer_policy is None:
                producer_policy = policy
            elif policy != producer_policy:
                raise ValueError("shards used different normalization producer environments")
            shard = report["execution"]["shard"]
            identity = (shard["suite"], shard["index"])
            if identity in seen or not (shard["suite"] == "curated" and shard["index"] == 0 and shard["count"] == 1
                    or shard["suite"] == "whole" and shard["count"] == count):
                raise ValueError("duplicate or unexpected shard")
            seen.add(identity)
            rows.extend(report["cases"])
            if not report["execution_complete"]:
                errors.append(f"incomplete shard {identity}")
        except (OSError, ValueError, KeyError, TypeError) as error:
            errors.append(f"invalid report {directory}: {error}")
    required = {("curated", 0)} | {("whole", index) for index in range(count)}
    if seen != required:
        errors.append(f"missing shards: {sorted(required - seen)}")
    keys = [(row["kind"], row["case"]) for row in rows]
    inventory = {(row["kind"], row["case"]) for row in expected["inventory"]}
    complete = not errors and len(keys) == len(set(keys)) and set(keys) == inventory
    counts = {status: sum(row["status"] == status for row in rows) for status in STATUSES}
    return {"schema_version": 1, "execution": {**common, "normalizer_policy": producer_policy}, "full_inventory_complete": complete,
            "execution_complete": complete, "strict_pass": bool(complete and not counts["fail"] and not counts["xpass"]),
            "inventory_total": len(inventory), "total": len(rows), "status_counts": counts,
            "errors": errors, "cases": sorted(rows, key=lambda row: (row["kind"], row["case"])),
            "unattempted_cases": [{"kind": kind, "case": case} for kind, case in sorted(inventory - set(keys))]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    phase = sub.add_parser("process")
    phase.add_argument("--directory", type=Path, required=True)
    phase.add_argument("--cwd", type=Path, required=True)
    phase.add_argument("--timeout", type=positive_seconds, required=True)
    phase.add_argument("--separate-streams", action="store_true")
    phase.add_argument("command", nargs=argparse.REMAINDER)
    prepare = sub.add_parser("prepare")
    prepare.add_argument("--root", type=Path, default=ROOT)
    prepare.add_argument("--report", type=Path, required=True)
    prepare.add_argument("--suite", choices=("all", "curated", "whole"), default="all")
    prepare.add_argument("--shard-index", type=int, default=0)
    prepare.add_argument("--shard-count", type=int, default=1)
    prepare.add_argument("--timeout", type=positive_seconds, required=True)
    prepare.add_argument("--profile", choices=("debug", "release"), required=True)
    prepare.add_argument("--rustflags", required=True)
    prepare.add_argument("--strict", action="store_true")
    prepare.add_argument("--pinned", action="store_true")
    prepare.add_argument("--runtime-profile", choices=("core", "graphics"), default="core")
    prepare.add_argument("--package-policy", choices=("native", "portable"), default="native")
    run = sub.add_parser("run")
    run.add_argument("--root", type=Path, default=ROOT)
    run.add_argument("--report", type=Path, required=True)
    run.add_argument("--gnu", type=Path, required=True)
    run.add_argument("--rust", type=Path, required=True)
    run.add_argument("--rlib", type=Path, required=True)
    seal = sub.add_parser("seal-library")
    seal.add_argument("--root", type=Path, default=ROOT)
    seal.add_argument("--report", type=Path, required=True)
    seal.add_argument("--rlib", type=Path, required=True)
    join = sub.add_parser("merge")
    join.add_argument("--root", type=Path, default=ROOT)
    join.add_argument("--input-dir", type=Path, required=True)
    join.add_argument("--report", type=Path, required=True)
    join.add_argument("--shard-count", type=int, required=True)
    join.add_argument("--source-commit", required=True)
    join.add_argument("--timeout", type=positive_seconds, required=True)
    join.add_argument("--profile", choices=("debug", "release"), required=True)
    join.add_argument("--rustflags", required=True)
    join.add_argument("--runtime-profile", choices=("core", "graphics"), default="core")
    join.add_argument("--package-policy", choices=("native", "portable"), default="native")
    args = parser.parse_args()
    try:
        if args.action == "process":
            command = args.command[1:] if args.command[:1] == ["--"] else args.command
            receipt = run_process(command, cwd=args.cwd, timeout=args.timeout,
                                  directory=args.directory, combined=not args.separate_streams)
            code = receipt["exit_code"]
            return code if code >= 0 else 128 - code
        if args.action == "prepare":
            fresh_directory(args.report)
            contract = contract_at(args.root, suite=args.suite, index=args.shard_index, count=args.shard_count,
                timeout=args.timeout, profile=args.profile, rustflags=args.rustflags, strict=args.strict, pinned=args.pinned,
                runtime_profile=args.runtime_profile, package_policy=args.package_policy)
            atomic_json(args.report / "contract.json", contract)
            publish_report(args.report, contract, [])
            return 0
        if args.action == "seal-library":
            seal_library(args.root, args.report, args.rlib)
            return 0
        if args.action == "merge":
            fresh_directory(args.report)
            directories = sorted(path.parent for path in args.input_dir.rglob("contract.json"))
            report = merge(args.root, directories, count=args.shard_count, commit=args.source_commit,
                           timeout=args.timeout, profile=args.profile, rustflags=args.rustflags,
                           runtime_profile=args.runtime_profile, package_policy=args.package_policy)
            atomic_json(args.report / "summary.json", report)
            (args.report / "summary.md").write_text("# Pinned upstream exact union\n\n"
                + f"Full inventory complete: **{report['full_inventory_complete']}**\n"
                + f"Strict parity passed: **{report['strict_pass']}**\n\n"
                + "\n".join("- " + error for error in report["errors"]) + "\n\n"
                + "\n".join(f"- {row['kind']}/{row['case']}: {row['status']} {row['detail']}"
                              for row in report["cases"] if row["status"] not in {"pass", "skip"}) + "\n")
            return 0 if report["strict_pass"] else 1
        contract = json.loads((args.report / "contract.json").read_text())
        verify_contract(args.root, contract)
        checked_builds(args.report, contract)
        if Path((args.report / "library-build/stdout.log").read_text().strip()).resolve() != args.rlib.resolve():
            raise ValueError("library differs from actual Cargo artifact selection")
        capture_artifacts(args.report, args.gnu, args.rust, args.rlib)
        report = execute(args.root, args.report, contract, args.gnu.resolve(), args.rust.resolve())
        return 0 if report["strict_pass"] else 1
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Upstream evidence error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
