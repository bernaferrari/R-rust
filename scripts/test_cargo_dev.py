#!/usr/bin/env python3
"""Exercise the real wrapper without compiling or touching shared build caches."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


WRAPPER = Path(__file__).resolve().with_name("cargo_dev.sh")


class CargoDevOutputTests(unittest.TestCase):
    def check_status(self, status):
        with tempfile.TemporaryDirectory(prefix="rport-cargo-stdout-") as temporary:
            root = Path(temporary)
            commands = root / "bin"
            commands.mkdir()
            cargo = commands / "cargo"
            cargo.write_text(
                "#!/usr/bin/env python3\n"
                "import json,os,sys\n"
                "status=int(os.environ['RPORT_FAKE_CARGO_STATUS'])\n"
                "print(json.dumps({'reason':'build-finished','success':status==0,"
                "'arguments':sys.argv[1:]}))\n"
                "print('compiler diagnostic',file=sys.stderr)\n"
                "sys.exit(status)\n"
            )
            cargo.chmod(0o755)
            target = root / "target"
            (target / "debug" / "deps").mkdir(parents=True)
            environment = os.environ.copy()
            environment["PATH"] = str(commands) + os.pathsep + environment["PATH"]
            environment["CARGO_TARGET_DIR"] = str(target)
            environment["RPORT_FAKE_CARGO_STATUS"] = str(status)
            environment.pop("RPORT_CARGO_ARTIFACTS_JSON", None)
            arguments = ["test", "--no-run", "--message-format=json"]
            result = subprocess.run(
                ["bash", str(WRAPPER), *arguments],
                env=environment, capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, status)
            self.assertEqual(json.loads(result.stdout), {
                "reason": "build-finished", "success": status == 0,
                "arguments": arguments,
            })
            self.assertIn("compiler diagnostic", result.stderr)
            self.assertIn("Libraries, object files and incremental caches retained.", result.stderr)

    def test_json_stdout_on_success(self):
        self.check_status(0)

    def test_json_stdout_and_original_failure_status(self):
        self.check_status(17)


if __name__ == "__main__":
    unittest.main()
