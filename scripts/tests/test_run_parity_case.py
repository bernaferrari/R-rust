import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


RUNNER = Path(__file__).resolve().parents[1] / "run_parity_case.py"


@unittest.skipUnless(os.name == "posix", "the Bash parity harness uses POSIX groups")
class ParityDeadlineTests(unittest.TestCase):
    def invoke(self, directory, program, timeout="5", combined_log=None):
        marker = Path(directory) / "timed-out"
        arguments = [sys.executable, str(RUNNER), "--timeout", timeout,
                     "--timeout-marker", str(marker)]
        if combined_log is not None:
            arguments.extend(["--combined-log", str(combined_log)])
        result = subprocess.run(
            arguments + ["--", sys.executable, "-c", program],
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

    def test_combined_file_preserves_bytes_and_child_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            logfile = Path(directory) / "combined.log"
            result, marker = self.invoke(
                directory,
                "import os; os.write(1,b'output  \\x00'); os.write(2,b'error\\n\\n'); raise SystemExit(7)",
                combined_log=logfile,
            )
            self.assertEqual(result.returncode, 7)
            self.assertEqual(logfile.read_bytes(), b"output  \x00error\n\n")
            self.assertEqual(result.stdout, b"")
            self.assertEqual(result.stderr, b"")
            self.assertFalse(marker.exists())

    def test_existing_combined_file_is_not_overwritten_or_executed(self):
        with tempfile.TemporaryDirectory() as directory:
            logfile = Path(directory) / "combined.log"
            logfile.write_bytes(b"previous evidence")
            touched = Path(directory) / "started"
            result, marker = self.invoke(
                directory, f"from pathlib import Path; Path({str(touched)!r}).touch()",
                combined_log=logfile,
            )
            self.assertEqual(result.returncode, 2)
            self.assertEqual(logfile.read_bytes(), b"previous evidence")
            self.assertFalse(touched.exists())
            self.assertFalse(marker.exists())

    def test_file_deadline_returns_with_detached_descendant_holding_output(self):
        with tempfile.TemporaryDirectory() as directory:
            logfile = Path(directory) / "combined.log"
            pidfile = Path(directory) / "detached.pid"
            child = (
                "from pathlib import Path; import os,time; "
                f"Path({str(pidfile)!r}).write_text(str(os.getpid())); "
                "os.write(1,b'partial  \\x00'); os.write(2,b'error\\n'); time.sleep(30)"
            )
            parent = (
                "import subprocess,sys,time; "
                f"subprocess.Popen([sys.executable,'-c',{child!r}],start_new_session=True); "
                "time.sleep(30)"
            )
            started = time.monotonic()
            try:
                result, marker = self.invoke(directory, parent, timeout="1", combined_log=logfile)
                self.assertLess(time.monotonic() - started, 5)
                self.assertEqual(result.returncode, 124)
                self.assertEqual(marker.read_text(), "1\n")
                self.assertEqual(logfile.read_bytes(), b"partial  \x00error\n")
                self.assertEqual(result.stdout, b"")
                self.assertIn(b"TIMEOUT after 1s", result.stderr)
                self.assertTrue(pidfile.exists())
                # Detached children are outside the runner's owned group. The
                # file capture must finish without claiming to terminate them.
                os.kill(int(pidfile.read_text()), 0)
            finally:
                if pidfile.exists():
                    try:
                        os.kill(int(pidfile.read_text()), signal.SIGKILL)
                    except ProcessLookupError:
                        pass

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
