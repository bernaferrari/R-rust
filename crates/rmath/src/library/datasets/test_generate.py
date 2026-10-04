"""Tooling admission tests; these do not claim runtime dataset parity."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

DIRECTORY = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("datasets_generate", DIRECTORY / "generate.py")
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


class InventoryAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.assets = DIRECTORY / "assets"
        self.manifest = json.loads((self.assets / "inventory.json").read_text())

    def validate(self, manifest):
        GENERATOR.validate_inventory(manifest["objects"], manifest["topics"],
                                     manifest["index"], (self.assets / "Rdata.rdb").stat().st_size)

    def test_complete_pinned_inventory_and_artifact_hashes(self):
        self.validate(self.manifest)
        for name, expected in self.manifest["artifacts_sha256"].items():
            self.assertEqual(GENERATOR.digest(self.assets / name), expected, name)
        self.assertEqual(self.manifest["topics"]["BJsales"], ["BJsales", "BJsales.lead"])
        self.assertEqual(self.manifest["namespace_exports"], [])

    def test_missing_or_duplicate_object_rejected(self):
        for altered in (self.manifest["objects"][:-1],
                        self.manifest["objects"][:-1] + [self.manifest["objects"][0]]):
            fixture = copy.deepcopy(self.manifest)
            fixture["objects"] = altered
            with self.assertRaises(ValueError):
                self.validate(fixture)

    def test_incomplete_or_duplicate_topic_membership_rejected(self):
        fixture = copy.deepcopy(self.manifest)
        fixture["topics"]["BJsales"] = ["BJsales", "BJsales"]
        with self.assertRaises(ValueError):
            self.validate(fixture)

    def test_out_of_range_or_overlapping_key_rejected(self):
        for offset in ("-1", "99999999", "0"):
            fixture = copy.deepcopy(self.manifest)
            fixture["objects"][1]["offset"] = offset
            with self.assertRaises(ValueError):
                self.validate(fixture)

    def test_missing_or_duplicate_index_row_rejected(self):
        for rows in (self.manifest["index"][:-1],
                     self.manifest["index"][:-1] + [self.manifest["index"][0]]):
            fixture = copy.deepcopy(self.manifest)
            fixture["index"] = rows
            with self.assertRaises(ValueError):
                self.validate(fixture)


if __name__ == "__main__":
    unittest.main()
