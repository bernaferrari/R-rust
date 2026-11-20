"""Regression checks for attributing written Rust unsafe syntax."""

import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "unsafe_syntax_report.py"


@unittest.skipUnless(shutil.which("ast-grep"), "requires ast-grep")
class UnsafeSyntaxReportTest(unittest.TestCase):
    def test_production_excludes_only_test_items_and_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            files = {
                "src/lib.rs": """
unsafe fn runtime() {}
#[cfg(not(test))]
fn production() { unsafe {} }
#[cfg(any(test, feature = "runtime"))]
fn shared_configuration() { unsafe {} }
#[cfg(all(test, not(miri)))]
fn test_configuration() { unsafe {} }
#[test]
fn direct_test() { unsafe {} }
#[cfg(test)]
// Intervening attributes and comments belong to the same module.
#[path = "special/test_body.rs"]
mod test_body;
#[cfg(test)]
mod inline {
    mod fixture;
    unsafe fn inline_only() {}
}
#[cfg(test)]
#[path = "shared.rs"]
mod shared_test;
#[cfg(feature = "runtime")]
#[path = "shared.rs"]
mod shared_runtime;
""",
                "src/special/test_body.rs": 'unsafe fn fixture() {}\n#[path = "deep.rs"]\nmod deep;',
                "src/special/deep.rs": "fn fixture() { unsafe {} }",
                "src/inline/fixture.rs": "unsafe fn fixture() {}",
                "src/shared.rs": "unsafe fn shared() {}",
            }
            for filename, contents in files.items():
                file = root / filename
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_text(contents)

            command = [sys.executable, str(SCRIPT), "--source-root", str(root)]
            written = json.loads(subprocess.check_output(command + ["src"], text=True))
            production = json.loads(subprocess.check_output(
                command + ["--production", "src"], text=True,
            ))
            self.assertEqual(written["total"], 10)
            self.assertEqual(production["total"], 4)
            self.assertEqual(production["files"], {"src/lib.rs": 3, "src/shared.rs": 1})
            self.assertEqual(
                production["counts"], {"unsafe_blocks": 2, "unsafe_function_declarations": 2},
            )


if __name__ == "__main__":
    unittest.main()
