import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest


RUNNER = Path(__file__).resolve().parents[1] / "run_parity_case.py"


@unittest.skipUnless(os.name == "posix", "the Bash parity harness uses POSIX groups")
class ParityDeadlineTests(unittest.TestCase):
    def invoke(self, directory, program, timeout="5"):
        marker = Path(directory) / "timed-out"
        result = subprocess.run(
            [sys.executable, str(RUNNER), "--timeout", timeout,
             "--timeout-marker", str(marker), "--", sys.executable, "-c", program],
            capture_output=True, timeout=10, check=False,
        )
        return result, marker

    def test_exact_bytes_and_nonzero_exit_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            result, marker = self.invoke(
                directory,
                "import os; os.write(1,b'output  '); os.write(2,b'error\\n\\n'); raise SystemExit(7)",
            )
            self.assertEqual(result.returncode, 7)
            self.assertEqual(result.stdout, b"output  ")
            self.assertEqual(result.stderr, b"error\n\n")
            self.assertFalse(marker.exists())

    def test_child_exit_124_is_not_a_timeout(self):
        with tempfile.TemporaryDirectory() as directory:
            result, marker = self.invoke(directory, "raise SystemExit(124)")
            self.assertEqual(result.returncode, 124)
            self.assertFalse(marker.exists())

    def test_deadline_stops_a_term_resistant_child_and_descendant(self):
        with tempfile.TemporaryDirectory() as directory:
            heartbeat = Path(directory) / "heartbeat"
            child = (
                "import pathlib,signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); "
                f"p=pathlib.Path({str(heartbeat)!r}); "
                "exec('while True:\\n with p.open(\"ab\") as f: f.write(b\".\")\\n time.sleep(.02)')"
            )
            parent = (
                "import signal,subprocess,sys,time; signal.signal(signal.SIGTERM,signal.SIG_IGN); "
                f"subprocess.Popen([sys.executable,'-c',{child!r}]); time.sleep(30)"
            )
            result, marker = self.invoke(directory, parent, timeout="1")
            self.assertEqual(result.returncode, 124)
            self.assertEqual(marker.read_text(), "1\n")
            self.assertIn(b"TIMEOUT after 1s", result.stderr)
            self.assertGreater(heartbeat.stat().st_size, 0)
            after_stop = heartbeat.stat().st_size
            time.sleep(0.1)
            self.assertEqual(heartbeat.stat().st_size, after_stop)

    def test_invalid_deadline_does_not_start_the_child(self):
        for timeout in ["0", "-1", "nan", "inf", "bad"]:
            with self.subTest(timeout=timeout), tempfile.TemporaryDirectory() as directory:
                touched = Path(directory) / "started"
                result, marker = self.invoke(
                    directory, f"from pathlib import Path; Path({str(touched)!r}).touch()", timeout,
                )
                self.assertEqual(result.returncode, 2)
                self.assertFalse(touched.exists())
                self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main()
