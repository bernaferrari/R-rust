"""Release asset admission; real Node/browser execution is verified separately."""
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import optimize_wasm_runtime as optimization


class OptimizationTests(unittest.TestCase):
    def test_a_bad_archive_is_rejected_before_extracting_or_executing(self):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            directory = cache / "arm64-macos"
            directory.mkdir()
            (directory / "binaryen-version_133-arm64-macos.tar.gz").write_bytes(b"corrupted")
            with patch.object(optimization.platform, "system", return_value="Darwin"), \
                 patch.object(optimization.platform, "machine", return_value="arm64"), \
                 patch.object(optimization.subprocess, "check_output") as execute:
                with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                    optimization.binaryen(cache)
                execute.assert_not_called()

    def test_optimizer_failure_preserves_the_original_asset(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            wasm = package / "r_wasm_bg.wasm"
            wasm.write_bytes(b"original")
            with patch.object(optimization.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "wasm-opt")):
                with self.assertRaises(subprocess.CalledProcessError):
                    optimization.optimize(package, Path("wasm-opt"))
            self.assertEqual(wasm.read_bytes(), b"original")
            self.assertFalse((package / "rust-runtime-optimization.json").exists())

    def test_release_processing_preserves_input_features_and_records_exact_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            (package / "r_wasm_bg.wasm").write_bytes(b"original")
            executable = package / "wasm-opt"
            executable.write_bytes(b"pinned optimizer")

            def run(arguments, *, check):
                self.assertTrue(check)
                self.assertEqual(arguments[2], "-Oz")
                self.assertFalse(any(argument.startswith("--enable") or argument == "--all-features" for argument in arguments))
                Path(arguments[4]).write_bytes(b"new")

            with patch.object(optimization.subprocess, "run", side_effect=run):
                optimization.optimize(package, executable)
            import json
            receipt = json.loads((package / "rust-runtime-optimization.json").read_text())
            self.assertEqual(receipt["input"]["bytes"], 8)
            self.assertEqual(receipt["output"], {"bytes": 3, "sha256": optimization.digest(package / "r_wasm_bg.wasm")})
            self.assertEqual(receipt["optimizer_sha256"], optimization.digest(executable))


if __name__ == "__main__":
    unittest.main()
