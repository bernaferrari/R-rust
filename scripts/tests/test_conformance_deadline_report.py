"""Exercise the real Bash case/report functions with controlled subprocesses."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "posix", "the Bash parity harness uses POSIX groups")
class ConformanceDeadlineReportTests(unittest.TestCase):
    def run_fixture(self, normal_timeout=False, error_timeout=False, shard_index=0, shard_count=1):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for name in ["cases", "golden", "error_cases", "error_golden", "bin"]:
                (directory / name).mkdir()
            for name, body in [("001_first", "normal"), ("002_second", "normal")]:
                if normal_timeout and name == "001_first":
                    body = "hang"
                (directory / "cases" / f"{name}.R").write_text(body)
                (directory / "golden" / f"{name}.out").write_text("OK\n")
            (directory / "error_cases/001_error.R").write_text("hang-error" if error_timeout else "error")
            (directory / "error_golden/001_error.out").write_text("Error: expected\n")
            xfail = "001_first" if normal_timeout else "001_error"
            (directory / "xfail.tsv").write_text(f"{xfail}\tfixture\texpected semantic gap\n" if normal_timeout or error_timeout else "")
            oracle = directory / "bin/Rscript"
            oracle.write_text(
                f"#!{sys.executable}\nfrom pathlib import Path\nimport sys\n"
                "error = 'error' in Path(sys.argv[-1]).read_text()\n"
                "print('Error: expected' if error else 'OK')\nraise SystemExit(1 if error else 0)\n"
            )
            oracle.chmod(0o755)
            rust = directory / "bin/rust_runner"
            rust.write_text(
                f"#!{sys.executable}\nfrom pathlib import Path\nimport sys,time\n"
                "body=Path(sys.argv[-1]).read_text()\n"
                "if 'hang' in body: time.sleep(30)\n"
                "error='error' in body\nprint('Error: expected' if error else 'OK')\n"
                "raise SystemExit(1 if error else 0)\n"
            )
            rust.chmod(0o755)
            source = (ROOT / "scripts/conformance_parity.sh").read_text()
            # Use the exact production functions; exclude installation/build
            # orchestration so this test isolates timeout and result admission.
            functions = (
                source[source.index("check_unique_case_numbers() {"):source.index('RUSTFLAGS_FOR_BUILD=')]
                + source[source.index("normalize_output() {"):source.rindex('main "$@"')]
            )
            values = {
                "ROOT_DIR": ROOT, "CASES_DIR": directory / "cases",
                "GOLDEN_DIR": directory / "golden", "ERROR_CASES_DIR": directory / "error_cases",
                "ERROR_GOLDEN_DIR": directory / "error_golden", "XFAIL_FILE": directory / "xfail.tsv",
                "RESULTS_TSV": directory / "results.tsv", "INVENTORY_TSV": directory / "inventory.tsv",
                "REPORT_JSON": directory / "summary.json", "REPORT_MD": directory / "summary.md",
                "REPORT_DIR": directory, "RUST_BIN": rust, "MODE": "--check", "R_MAJ_MIN": "4.7",
                "STRICT": "1", "CASE_TIMEOUT": "2", "CASE_TIMEOUT_DETAIL": "",
                "SHARD_INDEX": str(shard_index), "SHARD_COUNT": str(shard_count),
                "EXECUTION_JSON": directory / "execution.json",
            }
            for name in ["results.tsv", "inventory.tsv"]:
                (directory / name).touch()
            script = directory / "fixture.sh"
            script.write_text(
                "set -euo pipefail\n"
                + "\n".join(f"{key}={shlex.quote(str(value))}" for key, value in values.items())
                + "\n" + functions + '\nmain\n'
            )
            result = subprocess.run(
                ["bash", str(script)], capture_output=True, timeout=30, check=False,
                env={**os.environ, "PATH": f"{directory / 'bin'}:{os.environ['PATH']}"},
            )
            report_path = directory / "summary.json"
            self.assertTrue(report_path.exists(), (result.stdout, result.stderr))
            report = json.loads(report_path.read_text())
            return result, report

    def test_normal_timeout_is_a_failure_even_when_case_is_xfail(self):
        result, report = self.run_fixture(normal_timeout=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"RUN 001_first: Rust", result.stdout)
        self.assertIn(b"INCOMPLETE:", result.stderr)
        self.assertFalse(report["execution_complete"])
        self.assertEqual(report["failed"], 1)
        self.assertEqual(report["expected_failures"], 0)
        self.assertEqual(report["timed_out"], 1)
        self.assertEqual(report["inventory_total"], 3)
        self.assertEqual(report["unattempted"], 2)

    def test_expected_error_timeout_is_a_failure_and_never_normalized(self):
        result, report = self.run_fixture(error_timeout=True)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(report["execution_complete"])
        self.assertEqual(report["passed"], 2)
        self.assertEqual(report["failed"], 1)
        self.assertEqual(report["expected_failures"], 0)
        self.assertEqual(report["timed_out"], 1)
        self.assertEqual(report["unattempted"], 0)

    def test_shards_execute_disjoint_original_cases_and_expected_errors(self):
        first, a = self.run_fixture(shard_index=0, shard_count=2)
        second, b = self.run_fixture(shard_index=1, shard_count=2)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(a["passed"], 2)
        self.assertEqual(b["passed"], 1)
        self.assertEqual(a["global_inventory_total"], 3)
        self.assertEqual(a["inventory_total"], 2)
        self.assertEqual(b["inventory_total"], 1)
        self.assertTrue(a["execution_complete"])
        self.assertFalse(a["full_inventory_complete"])
        self.assertIn(b"RUN 001_error: Rust (expected error)", first.stdout)
        self.assertNotIn(b"RUN 002_second:", first.stdout)
        self.assertNotIn(b"RUN 001_first:", second.stdout)

    def test_shard_timeout_accounts_only_for_unattempted_selected_cases(self):
        result, report = self.run_fixture(normal_timeout=True, shard_index=0, shard_count=2)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(report["inventory_total"], 2)
        self.assertEqual(report["global_inventory_total"], 3)
        self.assertEqual(report["unattempted_cases"], [{"case": "001_error", "kind": "error"}])
        self.assertFalse(report["execution_complete"])
        self.assertFalse(report["full_inventory_complete"])

    def test_completed_run_keeps_all_case_and_error_checks(self):
        result, report = self.run_fixture()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(report["execution_complete"])
        self.assertEqual(report["passed"], 3)
        self.assertEqual(report["failed"], 0)
        self.assertEqual(report["timed_out"], 0)
        self.assertEqual(report["unattempted_cases"], [])


if __name__ == "__main__":
    unittest.main()
