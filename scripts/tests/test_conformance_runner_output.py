"""Isolate the production runner's emission contract from interpreter semantics."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(shutil.which("rustc"), "runner requires the Rust compiler")
class RunnerEmissionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.directory.cleanup)
        directory = Path(cls.directory.name)
        stub = directory / "stub.rs"
        # A test double supplies exact pre-captured bytes. It makes no claim
        # about evaluating R; this test checks the real standalone runner's I/O.
        stub.write_text('''
pub mod android {
    pub enum RValue { Error(String), Value }
    pub struct Result { pub typed: RValue, pub output: String }
    pub struct RSession;
    impl RSession {
        pub fn new() -> Self { Self }
        pub fn enable_host_process_capabilities(&mut self) {}
        pub fn eval(&mut self, code: &str) -> Result {
            match code.strip_prefix("ERROR:") {
                Some(output) => Result { typed: RValue::Error(String::new()), output: output.into() },
                None => Result { typed: RValue::Value, output: code.into() },
            }
        }
    }
}
''', encoding="utf-8")
        subprocess.run(
            ["rustc", "--edition=2024", "--crate-type=rlib", "--crate-name=rmath",
             str(stub), "--out-dir", str(directory)], check=True, capture_output=True,
        )
        cls.binary = directory / "runner"
        source = Path(os.environ.get(
            "RPORT_CONFORMANCE_RUNNER_SOURCE", str(ROOT / "tests/conformance/src/main.rs"),
        ))
        subprocess.run(
            ["rustc", "--edition=2024", str(source), "--extern",
             f"rmath={directory / 'librmath.rlib'}", "-o", str(cls.binary)],
            check=True, capture_output=True,
        )

    def invoke(self, payload):
        case = Path(self.directory.name) / "case.R"
        case.write_bytes(payload)
        return subprocess.run([str(self.binary), str(case)], capture_output=True, check=False)

    def test_stdout_has_no_added_newline_or_trimming(self):
        result = self.invoke(b"output  ")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"output  ")
        self.assertEqual(result.stderr, b"")

    def test_error_stream_preserves_newlines_and_failure_status(self):
        result = self.invoke(b"ERROR:prior output  \nError: failed\n\n")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(result.stderr, b"prior output  \nError: failed\n\n")

    def test_an_empty_capture_emits_no_bytes(self):
        result = self.invoke(b"")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(result.stderr, b"")


if __name__ == "__main__":
    unittest.main()
