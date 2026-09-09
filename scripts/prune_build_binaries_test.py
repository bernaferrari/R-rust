import fcntl
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

SCRIPT = Path(__file__).with_name("prune_build_binaries.py")
WRAPPER = Path(__file__).with_name("cargo_dev.sh")


class PruneTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.target = Path(self.temp.name) / "target"
        self.deps = self.target / "debug" / "deps"
        self.deps.mkdir(parents=True)

    def binary(self, number):
        path = self.deps / f"example-{number:016x}"
        path.write_bytes(b"binary")
        path.chmod(0o755)
        os.utime(path, (number, number))
        return path

    def run_prune(self, *args):
        return subprocess.run(["python3", str(SCRIPT), "--target-dir", str(self.target), *args],
                              capture_output=True, text=True)

    def test_preview_and_apply_preserve_recent_protected_and_nonbinary_files(self):
        paths = [self.binary(i) for i in range(4)]
        library = self.deps / "libexample-0000000000000000.rlib"
        library.write_bytes(b"library")
        incremental = self.target / "debug" / "incremental"
        incremental.mkdir()
        (incremental / "cache").write_bytes(b"cache")
        artifacts = Path(self.temp.name) / "artifacts.jsonl"
        artifacts.write_text(json.dumps({"reason": "compiler-artifact", "filenames": [str(paths[0])]}))
        self.assertEqual(self.run_prune("--artifacts-json", str(artifacts)).returncode, 0)
        self.assertTrue(all(p.exists() for p in paths))
        self.assertEqual(self.run_prune("--artifacts-json", str(artifacts), "--apply").returncode, 0)
        self.assertFalse(paths[1].exists())
        self.assertTrue(all(p.exists() for p in (paths[0], paths[2], paths[3], library, incremental / "cache")))

    def test_active_cargo_lock_prevents_cleanup(self):
        paths = [self.binary(i) for i in range(3)]
        with (self.target / "debug" / ".cargo-lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertNotEqual(self.run_prune("--apply").returncode, 0)
        self.assertTrue(all(p.exists() for p in paths))

    def test_only_old_large_diagnostics_are_cleared(self):
        directory = self.target / "debug" / ".fingerprint" / "example"
        directory.mkdir(parents=True)
        stale, recent = directory / "output-old", directory / "output-new"
        for path in (stale, recent):
            with path.open("wb") as file:
                file.truncate(11 * 1024**2)
        old = time.time() - 8 * 86400
        os.utime(stale, (old, old))
        self.assertEqual(self.run_prune("--apply").returncode, 0)
        self.assertEqual(stale.stat().st_size, 0)
        self.assertGreater(recent.stat().st_size, 0)

    def test_wrapper_preserves_cargo_failure_and_explicit_target(self):
        binaries = [self.binary(i) for i in range(3)]
        bin_dir = Path(self.temp.name) / "bin"
        bin_dir.mkdir()
        cargo = bin_dir / "cargo"
        cargo.write_text("#!/bin/sh\nexit 7\n")
        cargo.chmod(0o755)
        env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}"}
        result = subprocess.run([str(WRAPPER), "test", "--target-dir", str(self.target)], env=env,
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 7)
        self.assertFalse(binaries[0].exists())
        self.assertTrue(binaries[2].exists())


if __name__ == "__main__":
    unittest.main()
