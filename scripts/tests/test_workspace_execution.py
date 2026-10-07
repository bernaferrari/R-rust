"""Use real Cargo artifacts to check complete and red-capable workspace joins."""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import workspace_execution as workspace
from scripts.conformance_cargo_artifact import select_artifact


class SharedArtifactTests(unittest.TestCase):
    def test_only_current_successful_build_admits_the_shared_library(self):
        with tempfile.TemporaryDirectory() as directory:
            emitted = Path(directory) / "emitted.so"
            unrelated = Path(directory) / "newer.so"
            emitted.touch()
            unrelated.touch()
            message = {"reason": "compiler-artifact", "target": {"name": "r_uniffi", "kind": ["cdylib", "lib"]},
                       "profile": {"test": False}, "executable": None, "filenames": [str(emitted)]}
            finished = {"reason": "build-finished", "success": True}
            lines = [json.dumps(message), json.dumps(finished)]
            self.assertEqual(select_artifact(lines, "r_uniffi", shared=True), str(emitted))
            for broken in ([json.dumps(message)], [json.dumps(message), json.dumps({**finished, "success": False})],
                           ["{broken"], [json.dumps({**message, "profile": {"test": True}}), json.dumps(finished)],
                           [json.dumps(message), json.dumps({**message, "filenames": [str(unrelated)]}), json.dumps(finished)]):
                with self.assertRaises(ValueError):
                    select_artifact(broken, "r_uniffi", shared=True)
            emitted.unlink()
            with self.assertRaises(ValueError):
                select_artifact(lines, "r_uniffi", shared=True)


class WorkspaceExecutionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temporary.name).resolve() / "source"
        cls.root.mkdir()
        (cls.root / "Cargo.toml").write_text('[workspace]\nmembers = ["alpha", "beta"]\nresolver = "3"\n')
        for package in ("alpha", "beta"):
            directory = cls.root / package
            (directory / "src").mkdir(parents=True)
            (directory / "tests").mkdir()
            (directory / "Cargo.toml").write_text(
                f'[package]\nname = "workspace_fixture_{package}"\nversion = "0.0.0"\nedition = "2024"\n')
            (directory / "src/lib.rs").write_text('''/// ```
/// assert_eq!(2 + 2, 4);
/// assert!(env!("CARGO_PKG_NAME").ends_with("beta") || std::env::var_os("RPORT_WORKSPACE_FIXTURE_DOC_FAIL").is_none());
/// ```
pub fn value() -> i32 { 4 }
#[test] fn unit_contract() { assert_eq!(value(), 4); }
''')
            (directory / "tests/probe.rs").write_text('''#[test] fn package_cwd() {
    assert!(std::path::Path::new("Cargo.toml").is_file());
}
#[test] fn failure_keeps_other_targets_visible() {
    assert!(env!("CARGO_PKG_NAME").ends_with("beta") || std::env::var_os("RPORT_WORKSPACE_FIXTURE_FAIL").is_none());
}
''')
        (cls.root / "scripts").mkdir()
        cls.cargo = str(cls.root / "scripts/cargo_dev.sh")
        # Reuse the session's existing Cargo target and mandated wrapper.
        Path(cls.cargo).write_text(f'#!/bin/sh\nexec "{workspace.CARGO}" "$@"\n')
        Path(cls.cargo).chmod(0o755)
        uniffi = cls.root / "scripts/generate_uniffi_bindings.sh"
        uniffi.write_text('#!/bin/sh\ntest "$1" = --check\n')
        uniffi.chmod(0o755)
        subprocess.run(["git", "init", "-q", str(cls.root)], check=True)
        subprocess.run(["git", "add", "."], cwd=cls.root, check=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                        "commit", "-qm", "fixture"], cwd=cls.root, check=True)
        cls.environment = patch.dict(os.environ, {"CARGO_TARGET_DIR": str(workspace.ROOT / "target")})
        cls.environment.start()
        cls.wrapper = patch.object(workspace, "CARGO", cls.cargo)
        cls.wrapper.start()
        cls.reports = Path(cls.temporary.name) / "reports"
        cls.reports.mkdir()
        for index in range(3):
            report = workspace.produce(cls.root, cls.reports / str(index), index, 3, 120)
            if not report["strict_pass"]:
                raise AssertionError(report)
        cls.rows = json.loads((cls.reports / "0/inventory.json").read_text())
        cls.commit = workspace.source_identity(cls.root)["source_commit"]

    @classmethod
    def tearDownClass(cls):
        cls.wrapper.stop()
        cls.environment.stop()
        cls.temporary.cleanup()

    def setUp(self):
        self.working = tempfile.TemporaryDirectory()
        self.addCleanup(self.working.cleanup)
        self.directory = Path(self.working.name) / "reports"
        shutil.copytree(self.reports, self.directory)

    def merge(self, directories=None):
        return workspace.merge(self.root, directories or [self.directory / str(i) for i in range(3)],
                               self.rows, 3, self.commit, 120)

    def mutate(self, index, change):
        path = self.directory / str(index) / "summary.json"
        report = json.loads(path.read_text())
        change(report)
        path.write_text(json.dumps(report))

    def test_real_workspace_executes_every_target_and_doc_phase_once(self):
        report = self.merge()
        self.assertTrue(report["strict_pass"], report)
        self.assertEqual(report["inventory_total"], 6)
        self.assertEqual([len(workspace.partition(self.rows, index, 3)) for index in range(3)], [2, 2, 2])
        self.assertEqual(len({row["id"] for index in range(3) for row in workspace.partition(self.rows, index, 3)}), 6)
        self.assertEqual([row["phase"] for row in workspace.partition(self.rows, 2, 3)], ["doc", "doc"])

    def test_invalid_partition(self):
        for index, count in ((0, 1), (-1, 3), (3, 3), (0, 6), (True, 3), (0, True)):
            with self.assertRaises(ValueError):
                workspace.partition(self.rows, index, count)

    def test_missing_and_duplicate_shards_are_not_complete(self):
        for directories in ([self.directory / "0", self.directory / "1"],
                            [self.directory / "0", self.directory / "0", self.directory / "2"]):
            report = self.merge(directories)
            self.assertFalse(report["execution_complete"])
            self.assertFalse(report["strict_pass"])
            self.assertTrue(report["unverified_targets"])

    def test_omitted_duplicate_and_out_of_partition_executables_rejected(self):
        original = (self.directory / "0/summary.json").read_text()
        for change in (lambda report: report["rows"].pop(),
                       lambda report: report["rows"].append(copy.deepcopy(report["rows"][0])),
                       lambda report: report["rows"].__setitem__(0, json.loads((self.directory / "1/summary.json").read_text())["rows"][0])):
            (self.directory / "0/summary.json").write_text(original)
            self.mutate(0, change)
            self.assertFalse(self.merge()["strict_pass"])

    def test_source_toolchain_profile_or_feature_mismatch_rejected(self):
        original = (self.directory / "0/summary.json").read_text()
        for key, value in (("source_commit", "c" * 40), ("source_sha256", "d" * 64),
                           ("rustc", "another compiler"), ("build", ["test", "--release"]),
                           ("timeout_seconds", 999), ("shard_count", 4)):
            (self.directory / "0/summary.json").write_text(original)
            self.mutate(0, lambda report: report["profile"].__setitem__(key, value))
            self.assertFalse(self.merge()["strict_pass"], key)
        (self.directory / "0/summary.json").write_text(original)
        self.mutate(0, lambda report: report.__setitem__("actual_cargo_features", {}))
        self.assertFalse(self.merge()["strict_pass"])

    def test_tampered_logs_and_false_completion_rejected(self):
        path = self.directory / "0/build/stdout.log"
        original = path.read_bytes()
        path.write_bytes(original + b"changed\n")
        self.assertFalse(self.merge()["strict_pass"])
        path.write_bytes(original)
        self.mutate(0, lambda report: report.__setitem__("execution_complete", False))
        self.assertFalse(self.merge()["strict_pass"])

    def test_actual_failure_runs_remaining_targets_and_has_complete_red_join(self):
        failure = Path(self.working.name) / "failure"
        # A failing integration must not hide another package's assigned target.
        with patch.dict(os.environ, {"RPORT_WORKSPACE_FIXTURE_FAIL": "1"}):
            produced = workspace.produce(self.root, failure, 0, 3, 120)
        self.assertTrue(produced["execution_complete"])
        self.assertFalse(produced["strict_pass"])
        self.assertEqual(len(produced["rows"]), 2)
        self.assertEqual([row["process"]["exit_code"] == 0 for row in produced["rows"]], [False, True])
        report = self.merge([failure, self.directory / "1", self.directory / "2"])
        self.assertTrue(report["execution_complete"], report)
        self.assertFalse(report["strict_pass"])

    def test_unexecuted_docs_or_uniffi_never_claim_complete(self):
        self.mutate(2, lambda report: report["phases"].pop("uniffi"))
        self.assertFalse(self.merge()["execution_complete"])

    def test_failed_doctest_keeps_other_packages_and_uniffi_in_execution(self):
        failure = Path(self.working.name) / "doc-failure"
        with patch.dict(os.environ, {"RPORT_WORKSPACE_FIXTURE_DOC_FAIL": "1"}):
            produced = workspace.produce(self.root, failure, 2, 3, 120)
        self.assertTrue(produced["execution_complete"], produced)
        self.assertFalse(produced["strict_pass"])
        log = (failure / "doctests/combined.log").read_text()
        self.assertIn("test result: FAILED", log)
        self.assertIn("test result: ok", log)
        self.assertEqual(produced["phases"]["uniffi"]["exit_code"], 0)
        report = self.merge([self.directory / "0", self.directory / "1", failure])
        self.assertTrue(report["execution_complete"], report)
        self.assertFalse(report["strict_pass"])

    def test_group_deadline_publishes_incomplete_evidence(self):
        timeout = Path(self.working.name) / "timeout"
        report = workspace.produce(self.root, timeout, 0, 3, 0.0001)
        self.assertFalse(report["execution_complete"])
        self.assertFalse(report["strict_pass"])
        self.assertTrue(report["errors"])
        self.assertTrue((timeout / "summary.json").is_file())

    def test_changed_tracked_source_and_wrong_requested_commit_rejected(self):
        path = self.root / "alpha/src/lib.rs"
        original = path.read_bytes()
        try:
            path.write_bytes(original + b"// changed\n")
            self.assertFalse(self.merge()["strict_pass"])
        finally:
            path.write_bytes(original)
        self.assertFalse(workspace.merge(self.root, [self.directory / str(i) for i in range(3)],
                                        self.rows, 3, "f" * 40, 120)["strict_pass"])

    def test_unsuccessful_or_malformed_cargo_stream_and_missing_binary_rejected(self):
        directory = self.directory / "0"
        metadata = json.loads((directory / "metadata/stdout.log").read_text())
        lines = (directory / "build/stdout.log").read_text().splitlines()
        for stream in (["{broken"], ['{"reason":"build-finished","success":false}'], []):
            with self.assertRaises(ValueError):
                workspace.cargo_artifacts(stream, metadata, self.root, require_files=False)
        with patch.object(Path, "is_file", return_value=False):
            with self.assertRaises(ValueError):
                workspace.cargo_artifacts(lines, metadata, self.root, require_files=True)

    def test_distinct_host_and_runtime_dependency_profiles_are_both_retained(self):
        directory = self.directory / "0"
        metadata = json.loads((directory / "metadata/stdout.log").read_text())
        lines = (directory / "build/stdout.log").read_text().splitlines()
        executables, original = workspace.cargo_artifacts(lines, metadata, self.root, require_files=True)
        message = next(json.loads(line) for line in lines if line.startswith("{") and
                       json.loads(line).get("reason") == "compiler-artifact" and
                       not json.loads(line)["profile"]["test"])
        alternate = copy.deepcopy(message)
        alternate["profile"]["debuginfo"] = 0
        alternate["features"] = ["distinct-host-feature"]
        emitted, features = workspace.cargo_artifacts([json.dumps(alternate), *lines], metadata,
                                                      self.root, require_files=True)
        self.assertEqual(emitted, executables)
        self.assertEqual(len(features), len(original) + 1)
        self.assertTrue(any(row["features"] == ["distinct-host-feature"] for row in features.values()))


if __name__ == "__main__":
    unittest.main()
