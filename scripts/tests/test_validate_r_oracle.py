#!/usr/bin/env python3
from __future__ import annotations

import copy
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_r_oracle", ROOT / "scripts" / "validate_r_oracle.py"
)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)


class ManifestValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.manifest = validator.load_manifest(ROOT / "oracle" / "r-oracle.json")

    def write(self, manifest: dict) -> Path:
        temporary = tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False)
        json.dump(manifest, temporary)
        temporary.close()
        self.addCleanup(Path(temporary.name).unlink, missing_ok=True)
        return Path(temporary.name)

    def test_repository_manifest_is_valid(self) -> None:
        self.assertEqual(self.manifest["schema_version"], 1)

    def test_floating_commit_is_rejected(self) -> None:
        changed = copy.deepcopy(self.manifest)
        changed["source"]["commit"] = "trunk"
        with self.assertRaisesRegex(validator.ManifestError, "exact lowercase 40-hex commit"):
            validator.load_manifest(self.write(changed))

    def test_archive_must_name_the_pinned_commit(self) -> None:
        changed = copy.deepcopy(self.manifest)
        changed["source"]["archive_url"] = "https://github.com/wch/r-source/archive/trunk.tar.gz"
        with self.assertRaisesRegex(validator.ManifestError, "embed the exact source.commit"):
            validator.load_manifest(self.write(changed))

    def test_archive_requires_full_sha256(self) -> None:
        changed = copy.deepcopy(self.manifest)
        changed["source"]["archive_sha256"] = "8ee0845"
        with self.assertRaisesRegex(validator.ManifestError, "exact lowercase SHA-256"):
            validator.load_manifest(self.write(changed))

    def test_runtime_version_requires_number_and_status(self) -> None:
        changed = copy.deepcopy(self.manifest)
        changed["runtime"]["version"] = "devel"
        with self.assertRaisesRegex(validator.ManifestError, "exact version number and status"):
            validator.load_manifest(self.write(changed))

    def test_runtime_requires_matching_provenance_and_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            prefix = Path(temporary)
            rscript = prefix / "bin" / "Rscript"
            rscript.parent.mkdir()
            rscript.write_text(
                "#!/bin/sh\nprintf '%s\\n' '4.7.0' 'Under development (unstable)' "
                "'Unsuffered Consequences' '90451'\n",
                encoding="utf-8",
            )
            os.chmod(rscript, 0o755)
            manifest_path = ROOT / "oracle" / "r-oracle.json"
            (prefix / ".rport-oracle-manifest.sha256").write_text(
                validator.manifest_digest(manifest_path) + "\n", encoding="utf-8"
            )
            validator.verify_runtime(self.manifest, manifest_path, str(rscript))

    def test_unmarked_runtime_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            rscript = Path(temporary) / "bin" / "Rscript"
            rscript.parent.mkdir()
            rscript.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            os.chmod(rscript, 0o755)
            with self.assertRaisesRegex(validator.ManifestError, "no pinned-oracle provenance marker"):
                validator.verify_runtime(
                    self.manifest, ROOT / "oracle" / "r-oracle.json", str(rscript)
                )


if __name__ == "__main__":
    unittest.main()
