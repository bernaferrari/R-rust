"""Tooling admission tests with explicit fake engines; no GNU/runtime parity claim."""
from __future__ import annotations

import copy
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from scripts import upstream_execution as execution


class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_exact_streams_and_successful_finish_are_durable(self):
        phase = self.root / "phase"
        receipt = execution.run_process([sys.executable, "-c", "import os;os.write(1,b'a');os.write(2,b'b\\n')"],
            cwd=self.root, timeout=2, directory=phase)
        self.assertTrue(receipt["execution_complete"])
        self.assertEqual((phase / "combined.log").read_bytes(), b"ab\n")
        events = [json.loads(line) for line in (phase / "events.jsonl").read_text().splitlines()]
        self.assertEqual([row["event"] for row in events], ["START", "FINISH"])
        self.assertEqual(events[-1]["state"], "finished")

    def test_nonzero_exit_is_completed_execution_not_a_pass(self):
        receipt = execution.run_process([sys.executable, "-c", "raise SystemExit(7)"],
            cwd=self.root, timeout=2, directory=self.root / "phase")
        self.assertEqual(receipt["exit_code"], 7)
        self.assertTrue(receipt["execution_complete"])

    def test_separate_streams_preserve_legacy_contract(self):
        phase = self.root / "phase"
        execution.run_process([sys.executable, "-c", "import os;os.write(1,b'a');os.write(2,b'b')"],
            cwd=self.root, timeout=2, directory=phase, combined=False)
        self.assertEqual((phase / "stdout.log").read_bytes(), b"a")
        self.assertEqual((phase / "stderr.log").read_bytes(), b"b")

    def test_timeout_kills_owned_descendant_and_keeps_incomplete_receipt(self):
        marker = self.root / "escaped"
        script = ("import os,time,signal;pid=os.fork();"
                  "signal.signal(signal.SIGTERM,signal.SIG_IGN);"
                  f"time.sleep(.5);open({str(marker)!r},'w').write('escaped') if pid==0 else time.sleep(5)")
        phase = self.root / "phase"
        receipt = execution.run_process([sys.executable, "-c", script], cwd=self.root,
                                       timeout=.05, directory=phase)
        time.sleep(.6)
        self.assertFalse(marker.exists())
        self.assertEqual(receipt["state"], "timeout")
        self.assertFalse(receipt["execution_complete"])
        self.assertEqual(receipt["exit_code"], 124)

    def test_term_cancellation_preserves_started_phase_and_reaps_child(self):
        phase = self.root / "phase"
        process = subprocess.Popen([sys.executable, str(execution.ROOT / "scripts/upstream_execution.py"),
            "process", "--directory", str(phase), "--cwd", str(self.root), "--timeout", "10", "--",
            sys.executable, "-c", "import time;time.sleep(10)"], stdout=subprocess.DEVNULL)
        self.addCleanup(lambda: process.poll() is None and process.kill())
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            if (phase / "process.json").exists() and json.loads((phase / "process.json").read_text()).get("pid"):
                break
            time.sleep(.01)
        else:
            self.fail("fake owned child never started")
        process.send_signal(signal.SIGTERM)
        self.assertEqual(process.wait(timeout=3), 143)
        receipt = json.loads((phase / "process.json").read_text())
        self.assertEqual(receipt["state"], "cancelled")
        self.assertFalse(receipt["execution_complete"])
        with self.assertRaises(ProcessLookupError):
            os.kill(receipt["pid"], 0)

    def test_invalid_deadlines_rejected_before_process_or_output(self):
        for value in (0, -1, math.inf, math.nan, "bad"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                execution.run_process([sys.executable, "-c", "raise SystemExit(0)"],
                    cwd=self.root, timeout=value, directory=self.root / "phase")
            self.assertFalse((self.root / "phase").exists())

    def test_phase_reuse_and_launch_failure_do_not_claim_completion(self):
        receipt = execution.run_process(["/not/a/real/engine"], cwd=self.root,
            timeout=2, directory=self.root / "phase")
        self.assertEqual(receipt["state"], "launch_error")
        self.assertFalse(receipt["execution_complete"])
        with self.assertRaises(FileExistsError):
            execution.run_process([sys.executable], cwd=self.root, timeout=2, directory=self.root / "phase")


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for path in ("oracle", "tests/upstream-core/cases", "tests/upstream-r/vendor", "crates/fake/src"):
            (self.root / path).mkdir(parents=True)
        manifest = execution.ROOT / "oracle/r-oracle.json"
        (self.root / "oracle/r-oracle.json").write_bytes(manifest.read_bytes())
        self.oracle = json.loads(manifest.read_text())
        (self.root / "tests/upstream-core/cases/001.R").write_text("same\n")
        (self.root / "tests/upstream-core/cases/002.R").write_text("different\n")
        (self.root / "tests/upstream-core/xfail.tsv").write_text("")
        self.corpus = self.root / "tests/upstream-r"
        inventory = ["# r-source commit\t" + self.oracle["source"]["commit"]]
        dispositions = []
        for name, code, status in (("first.R", "same\n", "pass"), ("last.R", "same\n", "pass"),
                                   ("skip.R", "never\n", "skip")):
            path = self.corpus / "vendor" / name
            path.write_text(code)
            inventory.append(name + "\t" + execution.file_hash(path))
            dispositions.append(name + "\t" + status + ("\trport-test.1\tdeclared" if status == "skip" else "\t-\t-"))
        (self.corpus / "inventory.tsv").write_text("\n".join(inventory) + "\n")
        (self.corpus / "dispositions.tsv").write_text("\n".join(dispositions) + "\n")
        (self.root / "crates/fake/src/lib.rs").write_text("// fake compiled input\n")
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        # The tests do not need a commit or user Git identity. Only the source
        # revision query is mocked; actual input hashes are recomputed.
        original = subprocess.check_output
        def command_output(command, **kwargs):
            return "a" * 40 + "\n" if command[1:] == ["rev-parse", "HEAD"] else original(command, **kwargs)
        self.patch = patch.object(execution.subprocess, "check_output", side_effect=command_output)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.engines = self.root / "fake-engines"
        self.engines.mkdir()
        self.gnu, self.rust = self.engines / "gnu", self.engines / "rust"
        for path, suffix in ((self.gnu, "gnu"), (self.rust, "rust")):
            path.write_text("#!/usr/bin/env python3\nfrom pathlib import Path\nimport sys\n"
                "code=Path(sys.argv[-1]).read_text().strip()\n"
                + f"print(code if code!='different' else '{suffix}')\n")
            path.chmod(0o755)
        self.rlib = self.engines / "fake.rlib"
        self.rlib.write_bytes(b"explicit fake test artifact")

    def contract(self, suite="all", index=0, count=1):
        return execution.contract_at(self.root, suite=suite, index=index, count=count,
            timeout=2, profile="release", rustflags="-Awarnings", strict=True, pinned=True)

    def report(self, name, suite, index=0, count=1):
        directory = self.root / name
        execution.fresh_directory(directory)
        contract = self.contract(suite, index, count)
        execution.atomic_json(directory / "contract.json", contract)
        # Explicit fake build/oracle processes exercise evidence admission;
        # they never serve as a runtime compatibility proof.
        for phase in ("library-build", "runner-build", "oracle-validation"):
            code = f"print({str(self.rlib.resolve())!r})" if phase == "library-build" else "print('fake tooling build')"
            execution.run_process([sys.executable, "-c", code],
                                  cwd=self.root, timeout=2, directory=directory / phase,
                                  combined=phase != "library-build")
        execution.seal_library(self.root, directory, self.rlib)
        execution.capture_artifacts(directory, self.gnu, self.rust, self.rlib)
        return directory, execution.execute(self.root, directory, contract, self.gnu, self.rust)

    def reports(self):
        return [self.report("curated", "curated")[0], self.report("whole0", "whole", 0, 2)[0],
                self.report("whole1", "whole", 1, 2)[0]]

    def merge(self, paths):
        return execution.merge(self.root, paths, count=2, commit="a" * 40, timeout=2,
                               profile="release", rustflags="-Awarnings")

    def test_runtime_profiles_record_device_features_packages_and_backend(self):
        graphics = execution.runtime_configuration("graphics", "portable")
        self.assertEqual(graphics["device"]["width"], 504)
        self.assertEqual(graphics["package_policy"], "portable")
        self.assertEqual(graphics["numerical_backend"], "faer")
        self.assertIn("renderplot-device", graphics["cargo_features"])
        self.assertIsNone(execution.runtime_configuration("core", "native")["device"])
        for profile, policy in (("missing", "native"), ("core", "missing")):
            with self.assertRaises(ValueError):
                execution.runtime_configuration(profile, policy)
        contract = self.contract()
        contract["runtime"]["device"] = graphics["device"]
        with self.assertRaises(ValueError):
            execution.verify_contract(self.root, contract)

    def test_graphics_contract_requires_actual_cargo_device_receipt(self):
        directory, _ = self.report("no-device", "curated")
        contract = self.contract()
        contract["runtime"] = execution.runtime_configuration("graphics", "native")
        with self.assertRaisesRegex(ValueError, "Cargo feature/device receipt"):
            execution.checked_builds(directory, contract)

    def test_exact_original_partition_and_completed_failures_are_retained(self):
        result = self.merge(self.reports())
        self.assertTrue(result["execution_complete"])
        self.assertFalse(result["strict_pass"])
        self.assertEqual(result["total"], 5)
        self.assertEqual(result["status_counts"], {"pass": 3, "fail": 1, "xfail": 0, "xpass": 0, "skip": 1})

    def test_downloaded_producer_paths_do_not_resolve_against_consumer_filesystem(self):
        paths = self.reports()
        original_resolve = Path.resolve
        producer_selected = str(self.rlib.resolve())

        def consumer_resolve(path, *args, **kwargs):
            # macOS remaps /home to /System/Volumes/Data/home. A downloaded
            # Linux producer path must retain its producer-side identity.
            if str(path) == producer_selected:
                return Path("/consumer/remapped") / path.name
            return original_resolve(path, *args, **kwargs)

        with patch.object(Path, "resolve", consumer_resolve):
            result = self.merge(paths)
        self.assertTrue(result["execution_complete"], result["errors"])
        self.assertFalse(result["strict_pass"])
        self.assertEqual(result["total"], 5)

    def test_normalization_is_exact_original_pipeline_including_host_byte_edges(self):
        # This differential test originally failed for the Python translation
        # on Unicode whitespace and embedded NUL; keep the original tools.
        old = r"""tr -d '\r' | sed 's/[[:space:]]*$//' | awk '/^Time elapsed:/ { next } { lines[++n] = $0 } END { while (n > 0 && lines[n] == "") n--; for (i = 1; i <= n; i++) print lines[i] }'"""
        for index, raw in enumerate((b"a\r \t\nTime elapsed:99\n\n", b"a\xe2\x80\x83\n", b"a\xc2\xa0\n",
                                    b"a\x00b\n", b"<environment: 0x123>\n", b"Error: x\n")):
            directory = self.root / ("normalization" + str(index))
            (directory / "gnu").mkdir(parents=True)
            (directory / "gnu/combined.log").write_bytes(raw)
            receipt = execution.run_normalization(directory, "gnu", execution.normalizer_policy(), 2)
            original = subprocess.run(["bash", "-o", "pipefail", "-c", old], input=raw,
                                      capture_output=True, timeout=2,
                                      env={**os.environ, "LANG": "C", "LC_ALL": "C", "LC_CTYPE": "C"})
            self.assertEqual((directory / "gnu-normalize/stdout.log").read_bytes(), original.stdout)
            self.assertEqual(receipt["process"]["exit_code"], original.returncode)
            self.assertEqual(receipt["input_sha256"], execution.file_hash(directory / "gnu/combined.log"))

    def test_latin1_bytes_are_normalized_under_an_explicit_byte_locale(self):
        directory = self.root / "latin1-normalization"
        (directory / "gnu").mkdir(parents=True)
        (directory / "gnu/combined.log").write_bytes(b"caf\xe9 \r\n")
        with patch.dict(os.environ, {"LANG": "en_US.UTF-8", "LC_ALL": "C.UTF-8", "LC_CTYPE": "C.UTF-8"}):
            policy = execution.normalizer_policy()
            self.assertEqual(policy["locale"], {"LANG": "C", "LC_ALL": "C", "LC_CTYPE": "C"})
            receipt = execution.run_normalization(directory, "gnu", policy, 2)
        self.assertEqual(receipt["process"]["exit_code"], 0)
        self.assertEqual((directory / "gnu-normalize/stdout.log").read_bytes(), b"caf\xe9\n")

    def test_fixture_tampering_and_extra_vendor_files_fail_before_execution(self):
        contract = self.contract()
        source = self.corpus / "vendor/first.R"
        source.write_text("changed\n")
        with self.assertRaises(ValueError):
            execution.verify_contract(self.root, contract)
        source.write_text("same\n")
        (self.corpus / "vendor/extra.R").write_text("extra\n")
        with self.assertRaises(ValueError):
            self.contract()

    def test_changed_library_during_runner_compilation_cannot_borrow_seal(self):
        directory, report = self.report("sealed", "curated")
        self.rlib.write_bytes(b"later unrelated library")
        with self.assertRaisesRegex(ValueError, "library"):
            execution.capture_artifacts(directory, self.gnu, self.rust, self.rlib)

    def test_archived_normalizer_tampering_is_not_admitted(self):
        paths = self.reports()
        output = paths[0] / "cases/curated-001.R/gnu-normalize/stdout.log"
        output.write_bytes(b"changed normalization\n")
        self.assertFalse(self.merge(paths)["execution_complete"])

    def test_aggregator_environment_is_separate_but_producers_must_match(self):
        paths = self.reports()
        with patch.object(execution, "normalizer_policy", side_effect=ValueError("aggregator has no tools")):
            self.assertTrue(self.merge(paths)["execution_complete"])
        directory = paths[0]
        contract = json.loads((directory / "contract.json").read_text())
        contract["normalizer_policy"]["locale"]["LANG"] = "different"
        execution.atomic_json(directory / "contract.json", contract)
        summary = json.loads((directory / "summary.json").read_text())
        summary["execution"] = contract
        execution.atomic_json(directory / "summary.json", summary)
        self.assertFalse(self.merge(paths)["execution_complete"])

    def test_archived_tool_hash_tampering_is_not_admitted(self):
        paths = self.reports()
        path = paths[0] / "normalizer-tools/awk"
        path.write_bytes(b"different awk")
        self.assertFalse(self.merge(paths)["execution_complete"])

    def test_normalizer_failure_is_harness_failure_even_for_declared_xfail(self):
        row = {"disposition": "xfail"}
        phases = [{"engine": engine, "process": {"state": "finished", "exit_code": 0, "execution_complete": True}}
                  for engine in ("gnu", "rust")]
        norms = [{"engine": engine, "process": {"state": "finished", "exit_code": 2, "execution_complete": True}}
                 for engine in ("gnu", "rust")]
        self.assertEqual(execution.verdict(row, phases, self.root, norms), ("fail", "normalization harness failure"))

    def test_source_change_after_build_rejects_stale_runner(self):
        contract = self.contract()
        (self.root / "crates/fake/src/lib.rs").write_text("changed\n")
        with self.assertRaisesRegex(ValueError, "source"):
            execution.verify_contract(self.root, contract)

    def test_empty_invalid_partition_and_nonempty_output_rejected(self):
        rows = execution.catalog(self.root)
        for index, count in ((0, 0), (-1, 1), (0, 4), (True, 1)):
            with self.assertRaises(ValueError):
                execution.partition(rows, "whole", index, count)
        directory = self.root / "output"
        execution.fresh_directory(directory)
        (directory / "existing").write_text("data")
        with self.assertRaises(ValueError):
            execution.fresh_directory(directory)

    def test_missing_duplicate_extra_and_wrong_commit_never_complete_union(self):
        paths = self.reports()
        for candidate in (paths[:2], paths + [paths[0]]):
            self.assertFalse(self.merge(candidate)["execution_complete"])
        result = execution.merge(self.root, paths, count=2, commit="b" * 40, timeout=2,
                                  profile="release", rustflags="-Awarnings")
        self.assertFalse(result["execution_complete"])

    def test_tampered_logs_artifact_status_and_receipt_rejected(self):
        paths = self.reports()
        directory = paths[0]
        for relative in ("cases/curated-001.R/gnu/combined.log", "engines/rust",
                         "cases/curated-001.R/gnu/events.jsonl"):
            path = directory / relative
            original = path.read_bytes()
            path.write_bytes(original + b"tampered")
            self.assertFalse(self.merge(paths)["execution_complete"])
            path.write_bytes(original)
        summary = directory / "summary.json"
        original = summary.read_text()
        report = json.loads(original)
        report["cases"][0]["status"] = "fail"
        execution.atomic_json(summary, report)
        self.assertFalse(self.merge(paths)["execution_complete"])
        summary.write_text(original)

    def test_missing_build_and_partial_journal_never_complete_union(self):
        paths = self.reports()
        directory = paths[0]
        path = directory / "runner-build/process.json"
        original = path.read_text()
        receipt = json.loads(original)
        receipt.update(state="started", execution_complete=False)
        execution.atomic_json(path, receipt)
        self.assertFalse(self.merge(paths)["execution_complete"])
        path.write_text(original)
        journal = directory / "cases/curated-001.R/gnu/events.jsonl"
        original = journal.read_text()
        journal.write_text(original.splitlines()[0] + "\n")
        self.assertFalse(self.merge(paths)["execution_complete"])
        journal.write_text(original)

    def test_duplicate_extra_rows_or_changed_disposition_are_not_admitted(self):
        paths = self.reports()
        path = paths[0] / "summary.json"
        original = path.read_text()
        for change in ("duplicate", "extra", "skip"):
            report = json.loads(original)
            if change == "duplicate":
                report["cases"].append(copy.deepcopy(report["cases"][0]))
            elif change == "extra":
                report["cases"][0]["case"] = "not-in-inventory.R"
            else:
                report["cases"][0]["disposition"] = "skip"
            execution.atomic_json(path, report)
            self.assertFalse(self.merge(paths)["execution_complete"])
        path.write_text(original)

    def test_fixture_source_change_after_execution_invalidates_union(self):
        paths = self.reports()
        path = self.root / "crates/fake/src/lib.rs"
        path.write_text("// later implementation\n")
        self.assertFalse(self.merge(paths)["execution_complete"])

    def test_ordinary_gnu_error_is_completed_failure_without_running_rust(self):
        self.gnu.write_text("#!/usr/bin/env python3\nraise SystemExit(3)\n")
        directory, report = self.report("gnu-error", "curated")
        self.assertTrue(report["execution_complete"])
        self.assertFalse(report["strict_pass"])
        self.assertTrue(all(row["detail"] == "stock R exited non-zero" for row in report["cases"]))
        self.assertTrue(all(len(row["phases"]) == 1 for row in report["cases"]))

    def test_untyped_counts_and_boolean_policies_reject_ambiguous_json(self):
        paths = self.reports()
        path = paths[0] / "summary.json"
        original = path.read_text()
        for field in ("total", "inventory_total", "schema_version", "strict_pass", "execution_complete"):
            report = json.loads(original)
            report[field] = True if field in ("total", "inventory_total", "schema_version") else 1
            execution.atomic_json(path, report)
            self.assertFalse(self.merge(paths)["execution_complete"])
        path.write_text(original)
        contract = paths[0] / "contract.json"
        original = contract.read_text()
        report = json.loads(original)
        report["strict"] = 1
        execution.atomic_json(contract, report)
        summary = json.loads(path.read_text())
        summary["execution"] = report
        execution.atomic_json(path, summary)
        self.assertFalse(self.merge(paths)["execution_complete"])

    def test_partial_start_timeout_and_cancellation_never_become_xfail(self):
        entry = {"disposition": "xfail"}
        for state, code in (("timeout", 124), ("cancelled", 143)):
            phase = {"engine": "gnu", "process": {"state": state, "exit_code": code, "execution_complete": False}}
            self.assertEqual(execution.verdict(entry, [phase], self.root)[0], "fail")
        contract = self.contract()
        self.assertFalse(execution.summarize(contract, [])["execution_complete"])
        directory, report = self.report("partial", "curated")
        report["cases"].pop()
        execution.atomic_json(directory / "summary.json", report)
        self.assertFalse(self.merge([directory])["execution_complete"])

    def test_mixed_policy_or_source_contract_not_admitted(self):
        paths = self.reports()
        location = paths[0] / "contract.json"
        original = json.loads(location.read_text())
        for field, value in (("case_timeout_seconds", 3), ("profile", "debug"), ("source_commit", "b" * 40),
                             ("pinned_oracle_required", False), ("normalization", "weakened")):
            changed = copy.deepcopy(original)
            changed[field] = value
            execution.atomic_json(location, changed)
            self.assertFalse(self.merge(paths)["execution_complete"])
        execution.atomic_json(location, original)



class ShellIntegrationTests(unittest.TestCase):
    """Run the actual Bash orchestration with explicitly fake build tools."""
    def setUp(self):
        self.fixture = InventoryTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.root = self.fixture.root
        # Only this ephemeral fake-tool checkout receives an empty commit.
        # Child Bash processes do not inherit the unit test's revision mock.
        subprocess.run(["git", "-c", "user.name=Fake tooling fixture", "-c", "user.email=fake@example.invalid",
                        "commit", "--allow-empty", "-qm", "Explicit fake build tooling fixture"],
                       cwd=self.root, check=True)
        scripts = self.root / "scripts"
        scripts.mkdir()
        for name in ("upstream_core_slices.sh", "upstream_execution.py", "run_parity_case.py",
                     "conformance_artifacts.sh", "conformance_cargo_artifact.py",
                     "validate_upstream_r_tests.py", "validate_r_oracle.py"):
            (scripts / name).write_bytes((execution.ROOT / "scripts" / name).read_bytes())
        (self.root / "tests/conformance/src").mkdir(parents=True)
        (self.root / "tests/conformance/src/main.rs").write_text("// fake tooling runner source\n")
        tools = self.root / "fake-tools"
        tools.mkdir()
        (tools / "Rscript").write_bytes(self.fixture.gnu.read_bytes())
        (tools / "Rscript").chmod(0o755)
        wrapper = scripts / "cargo_dev.sh"
        wrapper.write_text("#!/usr/bin/env python3\nimport json\nfrom pathlib import Path\n"
            + f"artifact=Path({str(self.fixture.rlib)!r})\n"
            + "import sys\nfeatures=['default','faer','rust-backend']\n"
              "graphics='renderplot-device' in sys.argv\n"
              "if graphics: features+=['r-graphics-engine','renderplot-device']\n"
            + "print(json.dumps({'reason':'compiler-artifact','target':{'name':'rmath','kind':['rlib']},"
              "'profile':{'test':False},'features':features,'filenames':[str(artifact)]}))\n"
              "if graphics: print(json.dumps({'reason':'compiler-artifact','target':{'name':'r_graphics_engine','kind':['rlib']},"
              "'profile':{'test':False},'filenames':[str(artifact)]}))\n")
        wrapper.chmod(0o755)
        compiler = tools / "rustc"
        compiler.write_text("#!/usr/bin/env python3\nimport sys\nfrom pathlib import Path\n"
            + f"source=Path({str(self.fixture.rust)!r})\n"
            + "target=Path(sys.argv[sys.argv.index('-o')+1]);target.write_bytes(source.read_bytes());target.chmod(0o755)\n")
        compiler.chmod(0o755)
        self.environment = {**os.environ, "PATH": str(tools) + os.pathsep + os.environ["PATH"],
                            "RPORT_REQUIRE_PINNED_ORACLE": "0", "RPORT_CONFORMANCE_PROFILE": "release",
                            "CARGO_TARGET_DIR": str(self.root / "target"), "RUSTFLAGS": ""}

    def shell(self, *arguments):
        return subprocess.run(["bash", str(self.root / "scripts/upstream_core_slices.sh"),
                               "--runtime-profile", "core", *arguments], env=self.environment, cwd=self.root,
                              capture_output=True, timeout=10)

    def test_actual_shell_selects_emitted_artifact_and_durably_preserves_failure(self):
        directory = self.root / "report"
        result = self.shell("--strict", "--suite", "curated", "--timeout", "2", "--report", str(directory))
        self.assertEqual(result.returncode, 1, result.stderr)
        report = json.loads((directory / "summary.json").read_text())
        self.assertTrue(report["execution_complete"])
        self.assertFalse(report["strict_pass"])
        self.assertEqual(report["total"], 2)
        self.assertEqual((directory / "library-build/stdout.log").read_text().strip(), str(self.fixture.rlib))
        self.assertEqual(report["execution"]["profile"], "release")
        self.assertFalse(report["execution"]["pinned_oracle_required"])
        self.assertTrue((directory / "cases/curated-002.R/rust/combined.log").is_file())

    def test_graphics_shell_links_emitted_device_and_reports_portable_policy(self):
        directory = self.root / "graphics-report"
        result = self.shell("--strict", "--suite", "curated", "--timeout", "2", "--report", str(directory),
                            "--runtime-profile", "graphics", "--package-policy", "portable")
        self.assertEqual(result.returncode, 1, result.stderr)
        report = json.loads((directory / "summary.json").read_text())
        self.assertTrue(report["execution_complete"])
        self.assertEqual(report["execution"]["runtime"], execution.runtime_configuration("graphics", "portable"))
        command = json.loads((directory / "runner-build/process.json").read_text())["command"]
        self.assertIn("rport_renderplot", command)
        self.assertIn("r_graphics_engine=" + str(self.fixture.rlib), command)
        cargo = directory / "library-cargo.json"
        cargo.write_text(cargo.read_text().replace('"renderplot-device"', '"wrong-device"'))
        with self.assertRaisesRegex(ValueError, "Cargo features"):
            execution.checked_builds(directory, report["execution"])

    def test_shell_invalid_deadline_and_existing_report_fail_before_build(self):
        directory = self.root / "report"
        result = self.shell("--timeout", "0", "--report", str(directory))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((directory / "library-build").exists())
        directory.mkdir(exist_ok=True)
        (directory / "previous").write_text("keep")
        result = self.shell("--timeout", "2", "--report", str(directory))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((directory / "previous").read_text(), "keep")

    def test_shell_moving_compiler_input_cannot_publish_compatible_evidence(self):
        wrapper = self.root / "scripts/cargo_dev.sh"
        source = self.root / "crates/fake/src/lib.rs"
        wrapper.write_text(wrapper.read_text() + f"\nPath({str(source)!r}).write_text('// changed during build\\n')\n")
        directory = self.root / "report"
        result = self.shell("--strict", "--suite", "curated", "--timeout", "2", "--report", str(directory))
        self.assertNotEqual(result.returncode, 0)
        report = json.loads((directory / "summary.json").read_text())
        self.assertFalse(report["execution_complete"])
        self.assertEqual(report["total"], 0)
        self.assertFalse((directory / "cases").exists())

    def test_shell_shard_runs_only_original_selected_cases(self):
        directory = self.root / "report"
        result = self.shell("--strict", "--suite", "whole", "--shard-index", "1", "--shard-count", "2",
                            "--timeout", "2", "--report", str(directory))
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads((directory / "summary.json").read_text())
        self.assertEqual([(row["kind"], row["case"]) for row in report["cases"]], [("whole", "last.R")])
        self.assertTrue(report["execution_complete"])


if __name__ == "__main__":
    unittest.main()
