#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "validate_upstream_r_tests", ROOT / "scripts" / "validate_upstream_r_tests.py"
)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(validator)


class UpstreamCorpusValidationTests(unittest.TestCase):
    def test_repository_corpus_is_valid_and_complete(self) -> None:
        oracle = json.loads(
            (ROOT / "oracle" / "r-oracle.json").read_text(encoding="utf-8")
        )

        report = validator.validate_corpus(
            ROOT / "tests" / "upstream-r", oracle["source"]["commit"]
        )

        self.assertEqual(report.imported_files, 245)
        self.assertEqual(report.total, 70)
        self.assertEqual(report.skipped, 70)

    def make_corpus(
        self,
        *,
        disposition: str = "skip",
        owner: str = "rport-gap1",
        reason: str = "requires package loading",
    ) -> Path:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        vendor = root / "vendor"
        vendor.mkdir()
        case = vendor / "arith.R"
        case.write_text("print(1 + 1)\n", encoding="utf-8")
        digest = hashlib.sha256(case.read_bytes()).hexdigest()
        (root / "inventory.tsv").write_text(
            "# path\tsha256\n" f"arith.R\t{digest}\n", encoding="utf-8"
        )
        (root / "dispositions.tsv").write_text(
            "# path\tdisposition\towner bead\treason\n"
            f"arith.R\t{disposition}\t{owner}\t{reason}\n",
            encoding="utf-8",
        )
        return root

    def test_valid_corpus_reports_each_disposition(self) -> None:
        corpus = self.make_corpus()

        report = validator.validate_corpus(corpus)

        self.assertEqual(report.total, 1)
        self.assertEqual(report.skipped, 1)
        self.assertEqual(report.runnable, 0)

    def test_checksum_drift_is_rejected(self) -> None:
        corpus = self.make_corpus()
        (corpus / "vendor" / "arith.R").write_text("print(3)\n", encoding="utf-8")

        with self.assertRaisesRegex(validator.CorpusError, "checksum mismatch"):
            validator.validate_corpus(corpus)

    def test_uninventoried_vendor_file_is_rejected(self) -> None:
        corpus = self.make_corpus()
        (corpus / "vendor" / "extra.R").write_text("print(4)\n", encoding="utf-8")

        with self.assertRaisesRegex(validator.CorpusError, "not in inventory.tsv"):
            validator.validate_corpus(corpus)

    def test_missing_disposition_is_rejected(self) -> None:
        corpus = self.make_corpus()
        (corpus / "dispositions.tsv").write_text(
            "# path\tdisposition\towner bead\treason\n", encoding="utf-8"
        )

        with self.assertRaisesRegex(validator.CorpusError, "missing dispositions"):
            validator.validate_corpus(corpus)

    def test_skip_requires_owner_and_reason(self) -> None:
        corpus = self.make_corpus(owner="-", reason="-")

        with self.assertRaisesRegex(validator.CorpusError, "skip requires an owner bead"):
            validator.validate_corpus(corpus)

    def test_pass_cannot_hide_an_owner_or_reason(self) -> None:
        corpus = self.make_corpus(disposition="pass")

        with self.assertRaisesRegex(validator.CorpusError, "pass must use '-' owner and reason"):
            validator.validate_corpus(corpus)

    def test_xfail_is_counted_as_runnable(self) -> None:
        corpus = self.make_corpus(disposition="xfail")

        report = validator.validate_corpus(corpus)

        self.assertEqual(report.expected_failures, 1)
        self.assertEqual(report.runnable, 1)

    def test_nested_support_files_are_inventoried_without_dispositions(self) -> None:
        corpus = self.make_corpus()
        support = corpus / "vendor" / "pkgs" / "fixture" / "DESCRIPTION"
        support.parent.mkdir(parents=True)
        support.write_text("Package: fixture\n", encoding="utf-8")
        digest = hashlib.sha256(support.read_bytes()).hexdigest()
        inventory = corpus / "inventory.tsv"
        inventory.write_text(
            inventory.read_text(encoding="utf-8")
            + f"pkgs/fixture/DESCRIPTION\t{digest}\n",
            encoding="utf-8",
        )

        report = validator.validate_corpus(corpus)

        self.assertEqual(report.imported_files, 2)
        self.assertEqual(report.total, 1)


if __name__ == "__main__":
    unittest.main()
