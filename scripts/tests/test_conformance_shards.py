"""Check inventory partitioning and rejection of incomplete/mixed evidence."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts import conformance_shards as shards


class ConformanceShardTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for directory in ("cases", "golden", "error_cases", "error_golden"):
            (self.root / "tests/conformance" / directory).mkdir(parents=True)
        (self.root / "oracle").mkdir()
        (self.root / "oracle/r-oracle.json").write_text("{\"pinned\":true}\n")
        (self.root / "tests/conformance/xfail.tsv").write_text("")
        for kind, name in (("normal", "001_first"), ("normal", "002_second"), ("error", "001_first")):
            code = "cases" if kind == "normal" else "error_cases"
            expected = "golden" if kind == "normal" else "error_golden"
            (self.root / "tests/conformance" / code / f"{name}.R").write_text("1\n")
            (self.root / "tests/conformance" / expected / f"{name}.out").write_text("[1] 1\n")
        self.commit = "a" * 40
        self.source = patch.object(shards, "source_identity", return_value={
            "source_commit": self.commit, "source_sha256": "b" * 64,
        })
        self.source.start()
        self.addCleanup(self.source.stop)
        self.inventory = shards.inventory_at(self.root)

    def reports(self, count=2):
        reports = []
        for index in range(count):
            selected = shards.partition(self.inventory, index, count)
            rows = [{**row, "status": "pass", "detail": "", "domain": "fixture"} for row in selected]
            reports.append({
                "execution": shards.capture_contract(self.root, self.inventory, index=index, count=count,
                    profile="release", timeout=300.0, strict=True, pinned=True, engine="4.7"),
                "total": len(rows), "inventory_total": len(selected),
                "status_counts": {status: len(rows) if status == "pass" else 0 for status in shards.STATUSES},
                "execution_complete": True,
                "domains": [{"domain": "fixture", "cases": rows}],
            })
        return reports

    def merge(self, reports):
        return shards.merge_reports(self.root, reports, 2, self.commit)

    def test_all_original_identities_appear_once_in_balanced_partitions(self):
        # The normal/error directories may use the same filename identity.
        first, second = (shards.partition(self.inventory, i, 2) for i in range(2))
        self.assertEqual([len(first), len(second)], [2, 1])
        self.assertEqual({(row["kind"], row["case"]) for row in first + second},
                         {(row["kind"], row["case"]) for row in self.inventory})
        complete = self.merge(self.reports())
        self.assertTrue(complete["strict_pass"])
        self.assertEqual(complete["total"], 3)
        self.assertEqual(complete["status_counts"]["pass"], 3)

    def test_invalid_or_empty_partition_and_duplicate_identity_rejected(self):
        for index, count in ((0, 0), (-1, 2), (2, 2), (0, 4), (True, 2), (0, True)):
            with self.assertRaises(ValueError):
                shards.partition(self.inventory, index, count)
        with self.assertRaises(ValueError):
            shards.partition(self.inventory + [self.inventory[0]], 0, 2)

    def test_missing_shard_cannot_claim_complete(self):
        result = self.merge(self.reports()[:1])
        self.assertFalse(result["execution_complete"])
        self.assertFalse(result["strict_pass"])
        self.assertEqual(len(result["unattempted_cases"]), 1)
        self.assertIn("missing shards", " ".join(result["errors"]))

    def test_duplicate_shard_cannot_replace_missing_shard(self):
        result = self.merge([self.reports()[0]] * 2)
        self.assertFalse(result["strict_pass"])
        self.assertIn("duplicate shard", " ".join(result["errors"]))

    def test_duplicate_extra_or_other_shard_case_rejected(self):
        for bad_row in (self.reports()[0]["domains"][0]["cases"][0],
                        self.reports()[1]["domains"][0]["cases"][0],
                        {"kind": "normal", "case": "999_extra", "status": "pass", "detail": ""}):
            reports = self.reports()
            reports[0]["domains"][0]["cases"].append(copy.deepcopy(bad_row))
            result = self.merge(reports)
            self.assertFalse(result["strict_pass"])
            self.assertIn("out-of-partition", " ".join(result["errors"]))

    def test_source_or_policy_mismatch_is_not_joined(self):
        replacements = {"source_commit": "c" * 40, "source_sha256": "d" * 64,
            "corpus_sha256": "e" * 64, "oracle_manifest_sha256": "f" * 64,
            "strict": False, "pinned_oracle_required": False,
            "profile": "debug", "build_rustflags": "-C opt-level=0", "case_timeout_seconds": 600, "engine_major_minor": "4.6"}
        for field, value in replacements.items():
            with self.subTest(field=field):
                reports = self.reports()
                reports[0]["execution"][field] = value
                result = self.merge(reports)
                self.assertFalse(result["strict_pass"])
                self.assertIn("execution policy differs", " ".join(result["errors"]))

    def test_changed_golden_source_xfail_or_oracle_rejects_old_reports(self):
        for relative in ("tests/conformance/cases/001_first.R",
                         "tests/conformance/golden/001_first.out",
                         "tests/conformance/xfail.tsv", "oracle/r-oracle.json"):
            with self.subTest(path=relative):
                reports = self.reports()
                path = self.root / relative
                original = path.read_bytes()
                path.write_bytes(original + b"changed\n")
                self.assertFalse(self.merge(reports)["strict_pass"])
                path.write_bytes(original)

    def test_malformed_json_shapes_are_rejected_with_failure_evidence(self):
        for broken in ([], {"execution": []}, {"execution": None}):
            self.assertFalse(self.merge([broken])["strict_pass"])
        for detail in (None, 1, []):
            reports = self.reports()
            reports[0]["domains"][0]["cases"][0]["detail"] = detail
            self.assertFalse(self.merge(reports)["strict_pass"])

    def test_counts_and_completion_claims_are_recomputed(self):
        for field, value in (("total", 999), ("inventory_total", 999),
                             ("status_counts", {"pass": 999}), ("execution_complete", False)):
            with self.subTest(field=field):
                reports = self.reports()
                reports[0][field] = value
                self.assertFalse(self.merge(reports)["strict_pass"])

    def test_incomplete_or_timeout_runs_keep_missing_and_failed_evidence(self):
        reports = self.reports()
        reports[0]["domains"][0]["cases"].pop()
        reports[0].update(total=1, execution_complete=False,
            status_counts={status: int(status == "pass") for status in shards.STATUSES})
        incomplete = self.merge(reports)
        self.assertFalse(incomplete["execution_complete"])
        self.assertEqual(len(incomplete["unattempted_cases"]), 1)
        reports = self.reports()
        row = reports[0]["domains"][0]["cases"][0]
        row.update(status="fail", detail="timeout: original deadline exceeded")
        reports[0]["status_counts"].update({"pass": 1, "fail": 1})
        reports[0]["execution_complete"] = False
        timed = self.merge(reports)
        self.assertFalse(timed["execution_complete"])
        self.assertEqual(timed["timed_out"], 1)

    def test_semantic_gaps_are_complete_execution_but_never_strict_success(self):
        for status in ("fail", "xfail", "xpass", "skip"):
            with self.subTest(status=status):
                reports = self.reports()
                reports[0]["domains"][0]["cases"][0]["status"] = status
                reports[0]["status_counts"].update({"pass": 1, status: 1})
                result = self.merge(reports)
                self.assertTrue(result["execution_complete"])
                self.assertFalse(result["strict_pass"])

    def test_aggregation_checkout_must_match_requested_commit(self):
        result = shards.merge_reports(self.root, self.reports(), 2, "c" * 40)
        self.assertFalse(result["strict_pass"])

    def test_cli_writes_failure_evidence_when_all_shards_are_missing(self):
        # This invokes real identity discovery; no mocked helper crosses the subprocess.
        with tempfile.TemporaryDirectory() as output:
            directory = Path(output)
            result = subprocess.run([sys.executable, str(shards.ROOT / "scripts/conformance_shards.py"),
                "--root", str(shards.ROOT), "--input-dir", str(directory / "missing"),
                "--source-commit", "c" * 40, "--shard-count", "6",
                "--json", str(directory / "summary.json"), "--markdown", str(directory / "summary.md")],
                capture_output=True, check=False, timeout=30)
            self.assertEqual(result.returncode, 1, result.stderr)
            report = json.loads((directory / "summary.json").read_text())
            self.assertFalse(report["execution_complete"])
            self.assertFalse(report["strict_pass"])
            self.assertIn("missing shards", " ".join(report["errors"]))


if __name__ == "__main__":
    unittest.main()
