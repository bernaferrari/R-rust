import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from scripts.validate_embedded_r_sources import COMMIT, REPOSITORY, validate


class EmbeddedSourcesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.assets = self.root / 'crates/rmath/upstream'
        self.asset = self.assets / 'base/R/identity.R'
        self.asset.parent.mkdir(parents=True)
        self.asset.write_bytes(b'identity <- function(x) x\n')
        (self.assets / 'COPYING').write_bytes(b'GNU GPL fixture\n')
        self.rust = self.root / 'crates/rmath/src/lib.rs'
        self.rust.parent.mkdir(parents=True)
        self.rust.write_text('const SOURCE: &str = include_str!("../upstream/base/R/identity.R");\n')
        self.manifest = {
            'schema_version': 1, 'source': {'repository': REPOSITORY, 'commit': COMMIT},
            'license': {'spdx': 'GPL-2.0-or-later', 'path': 'COPYING',
                        'sha256': hashlib.sha256((self.assets / 'COPYING').read_bytes()).hexdigest()},
            'files': {'base/R/identity.R': {'source_path': 'src/library/base/R/identity.R',
                      'bytes': self.asset.stat().st_size, 'sha256': hashlib.sha256(self.asset.read_bytes()).hexdigest()}},
        }
        self.save()

    def save(self):
        (self.assets / 'manifest.json').write_text(json.dumps(self.manifest))

    def test_exact_packaged_input_is_authenticated_without_a_reference_checkout(self):
        self.assertFalse((self.root / 'r-source').exists())
        self.assertEqual(validate(self.root), {'files': 1, 'bytes': self.asset.stat().st_size, 'source_commit': COMMIT})

    def test_missing_packaged_file_rejects(self):
        self.asset.unlink()
        with self.assertRaises(FileNotFoundError):
            validate(self.root)

    def test_changed_R_bytes_reject(self):
        self.asset.write_bytes(b'identity <- function(x) NULL\n')
        with self.assertRaisesRegex(ValueError, 'content differs'):
            validate(self.root)

    def test_wrong_upstream_pin_rejects(self):
        self.manifest['source']['commit'] = '0' * 40
        self.save()
        with self.assertRaisesRegex(ValueError, 'provenance'):
            validate(self.root)

    def test_ignored_reference_input_rejects_even_with_packaged_assets_present(self):
        self.rust.write_text('const SOURCE: &str = include_str!("../../../r-source/file.R");\n')
        with self.assertRaisesRegex(ValueError, 'ignored r-source'):
            validate(self.root)

    def test_unreferenced_packaged_input_rejects(self):
        self.rust.write_text('// no source inputs\n')
        with self.assertRaisesRegex(ValueError, 'include paths differ'):
            validate(self.root)

    def test_extra_file_rejects(self):
        (self.assets / 'legacy.c').write_text('int unused;\n')
        with self.assertRaisesRegex(ValueError, 'files differ'):
            validate(self.root)

    def test_parent_path_rejects(self):
        self.manifest['files']['../escape.R'] = self.manifest['files'].pop('base/R/identity.R')
        self.save()
        with self.assertRaisesRegex(ValueError, 'invalid packaged R path'):
            validate(self.root)

    def test_changed_license_rejects(self):
        (self.assets / 'COPYING').write_text('different license\n')
        with self.assertRaisesRegex(ValueError, 'license content changed'):
            validate(self.root)


if __name__ == '__main__':
    unittest.main()
