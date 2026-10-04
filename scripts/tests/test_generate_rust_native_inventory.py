#!/usr/bin/env python3
"""Fake-tool admission tests only; no Rust or GNU runtime parity is tested."""
from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import unittest

from scripts.tests import test_join_gnu_r_api_evidence as fixture_module
import generate_rust_native_inventory as producer

joiner = producer.evidence


class ProducerAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        # Reuse only the authenticated synthetic census fixture, not its tests.
        fixture = fixture_module.SyntheticJoinAdmissionTests(methodName="runTest")
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        self.fixture = fixture
        self.root, self.source, self.census = fixture.root, fixture.source, fixture.census
        self.output = self.root / "generated"
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.make_executable(self.tools / "git", "import sys\nprint('a'*40 if sys.argv[1] == 'rev-parse' else '')\n")
        self.make_executable(self.tools / "rustc", "print('rustc synthetic\\nhost: synthetic-target')\n")
        (self.source / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "synthetic"\n')
        self.make_executable(self.source / "scripts/cargo_dev.sh", FAKE_WRAPPER)
        self.env = dict(os.environ, PATH=str(self.tools) + os.pathsep + os.environ["PATH"],
                        RUSTC=str(self.tools / "rustc"), CARGO_HOME=str(self.root / "cargo-home"),
                        CARGO_BUILD_TARGET="synthetic-target", FAKE_MODE="complete")
        for name in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"]:
            self.env.pop(name, None)

    def make_executable(self, path: Path, body: str) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"#!{sys.executable}\n" + body)
        path.chmod(0o755)

    def run_producer(self, **kwargs) -> dict:
        return producer.generate(self.census, self.output, self.source, self.fixture.manifest_path,
                                 environment=self.env, **kwargs)

    def rejected(self, mode: str, pattern: str) -> None:
        self.env["FAKE_MODE"] = mode
        with self.assertRaisesRegex((producer.InventoryError, joiner.EvidenceError), pattern):
            self.run_producer()
        self.assertFalse((self.output / "provenance.json").exists())
        self.assertTrue((self.output / "probe.log").is_file())
        self.assertFalse(json.loads((self.output / "failure.json").read_text())["execution_complete"])

    def test_completed_producer_is_joinable_and_retains_every_unsupported_row(self) -> None:
        provenance = self.run_producer()
        self.assertTrue(provenance["execution_complete"])
        self.assertIn("rust-toolchain.toml", provenance["source_files_sha256"])
        self.assertEqual(provenance["source_revision"], "a" * 40)
        result = joiner.join(self.census, self.output, self.root / "joined-generated",
                             self.fixture.manifest_path, self.output / "source",
                             Path(provenance["build_profile"]["compiled_artifact_path"]))
        self.assertEqual((result["native_rows"], result["unsupported_rows"]), (4, 4))
        self.assertFalse(result["behavior_assessed"])
        invocation = json.loads(next(line for line in (self.output / "probe.log").read_text().splitlines()
                                     if line.startswith('{"invocation":')))["invocation"]
        self.assertEqual(invocation, ["test", "-p", "rmath", "--lib", "--message-format=json", producer.EXPORTER,
                                      "--", "--exact", "--ignored", "--nocapture", "--test-threads=1"])

    def test_inherited_target_and_flags_are_not_overridden(self) -> None:
        self.env.update(CARGO_TARGET_DIR=str(self.root / "inherited-target"), RUSTFLAGS="-C opt-level=1")
        profile = self.run_producer()["build_profile"]
        self.assertEqual(profile["environment"]["CARGO_TARGET_DIR"], self.env["CARGO_TARGET_DIR"])
        self.assertEqual(profile["flags"], ["-C", "opt-level=1"])

    def test_explicit_target_changes_only_target_directory(self) -> None:
        self.env.update(CARGO_TARGET_DIR="original", CARGO_ENCODED_RUSTFLAGS="-C\x1fdebuginfo=1")
        selected = self.root / "explicit-target"
        profile = self.run_producer(target_dir=selected)["build_profile"]
        self.assertEqual(profile["environment"]["CARGO_TARGET_DIR"], str(selected))
        self.assertEqual(profile["flags"], ["-C", "debuginfo=1"])

    def test_source_mutation_rejects_publication(self) -> None:
        self.rejected("source-change", "sources changed")

    def test_toolchain_mutation_rejects_publication(self) -> None:
        self.rejected("toolchain-change", "sources changed")

    def test_exit_zero_without_complete_footer_is_not_completion(self) -> None:
        self.rejected("no-footer", "completed one-test footer")

    def test_failed_build_retains_logs(self) -> None:
        self.rejected("failed-build", "exited 7")

    def test_row_count_must_match_census(self) -> None:
        self.rejected("wrong-count", "row-count footer")

    def test_cargo_json_artifact_is_required(self) -> None:
        self.rejected("no-artifact", "compiler artifact")

    def test_malformed_artifact_is_typed_failure(self) -> None:
        self.rejected("malformed-artifact", "compiler artifact")

    def test_duplicate_resolver_rows_are_rejected(self) -> None:
        self.rejected("duplicate-row", "duplicate resolver key")

    def test_missing_resolver_row_is_rejected(self) -> None:
        self.rejected("missing-row", "resolver keys differ: missing=")

    def test_altered_exporter_input_is_rejected(self) -> None:
        self.rejected("input-change", "census input changed")

    def test_duplicate_completion_footer_is_rejected(self) -> None:
        self.rejected("duplicate-footer", "unique completed")

    def test_zero_test_footer_is_not_completion(self) -> None:
        self.rejected("zero-tests", "completed one-test")

    def test_duplicate_cargo_artifact_is_rejected(self) -> None:
        self.rejected("duplicate-artifact", "exactly one")

    def test_timeout_preserves_incomplete_evidence(self) -> None:
        self.env["FAKE_MODE"] = "timeout"
        with self.assertRaises(producer.InventoryError) as caught:
            self.run_producer(timeout=0.2)
        self.assertEqual(caught.exception.exit_code, 124)
        self.assertTrue((self.output / "timeout.marker").is_file())
        self.assertFalse((self.output / "provenance.json").exists())
        self.assertTrue(json.loads((self.output / "failure.json").read_text())["timeout"])

    def test_nonempty_output_is_untouched(self) -> None:
        self.output.mkdir()
        sentinel = self.output / "keep"
        sentinel.write_bytes(b"original")
        with self.assertRaisesRegex(producer.InventoryError, "new or empty"):
            self.run_producer()
        self.assertEqual(list(self.output.iterdir()), [sentinel])
        self.assertEqual(sentinel.read_bytes(), b"original")

    def test_invalid_deadline_does_not_start_producer(self) -> None:
        for deadline in [0, -1, float("inf"), float("nan")]:
            with self.subTest(deadline=deadline), self.assertRaisesRegex(producer.InventoryError, "positive and finite"):
                self.run_producer(timeout=deadline)
        self.assertFalse(self.output.exists())

    def test_tampered_census_is_rejected_before_build(self) -> None:
        (self.census / "syntax.rds").write_bytes(b"tampered")
        with self.assertRaisesRegex(joiner.EvidenceError, "census hash mismatch"):
            self.run_producer()
        self.assertFalse(self.output.exists())


FAKE_WRAPPER = r'''
import json, os, pathlib, sys, time
print(json.dumps({"invocation": sys.argv[1:]}), flush=True)
mode = os.environ["FAKE_MODE"]
if mode == "timeout":
    print("synthetic owned process waiting", flush=True)
    time.sleep(30)
if mode == "failed-build":
    print("synthetic build failed", flush=True)
    sys.exit(7)
source = pathlib.Path.cwd()
if mode == "source-change":
    (source / "crates/rmath/src/lib.rs").write_text("changed source")
if mode == "toolchain-change":
    (source / "rust-toolchain.toml").write_text("changed toolchain")
rows = pathlib.Path(os.environ["RPORT_NATIVE_CENSUS_INPUT"]).read_text().splitlines()[1:]
header = "dll\tinterface\tname\tnum_parameters\tresolver_status\tactual_interface\tactual_arity_kind\tactual_num_parameters\n"
values = [row + "\tunsupported\t\t\t\n" for row in rows]
if mode == "duplicate-row":
    values.append(values[0])
if mode == "missing-row":
    values.pop()
if mode == "input-change":
    pathlib.Path(os.environ["RPORT_NATIVE_CENSUS_INPUT"]).write_text("changed input")
pathlib.Path(os.environ["RPORT_NATIVE_RESOLVER_OUTPUT"]).write_text(header + "".join(values))
artifact = source / "synthetic-test-artifact"
artifact.write_bytes(b"explicit fake test artifact, no runtime semantics")
if mode != "no-artifact":
    artifact_metadata = {"reason": "compiler-artifact", "target": None if mode == "malformed-artifact" else {"name": "rmath"},
                      "profile": {"test": True, "opt_level": "0"}, "features": ["default"], "executable": str(artifact)}
    print(json.dumps(artifact_metadata))
    if mode == "duplicate-artifact":
        print(json.dumps(artifact_metadata))
print(f"Exported {len(rows) + (1 if mode == 'wrong-count' else 0)} native registration descriptors without invocation")
if mode != "no-footer":
    footer = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.00s"
    print(footer.replace("1 passed", "0 passed") if mode == "zero-tests" else footer)
    if mode == "duplicate-footer":
        print(footer)
'''


if __name__ == "__main__":
    unittest.main()
