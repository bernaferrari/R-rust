#!/usr/bin/env python3
"""Synthetic tooling-admission fixtures; these do not prove runtime parity."""
from __future__ import annotations

import copy
import csv
import importlib.util
import json
import io
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("join_gnu_r_api_evidence", ROOT / "scripts/join_gnu_r_api_evidence.py")
assert SPEC is not None and SPEC.loader is not None
joiner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(joiner)


class SyntheticJoinAdmissionTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.census = self.root / "census"
        self.resolver = self.root / "resolver"
        self.source = self.root / "source"
        self.output = self.root / "joined"
        for directory in [self.census, self.resolver, self.source]:
            directory.mkdir()
        self.manifest_path = ROOT / "oracle/r-oracle.json"
        manifest = joiner.load_manifest(self.manifest_path)
        self.native = []
        for interface, name, parameters in [(".C", "native", "1"), (".Fortran", "linear", "2"),
                                             (".Call", "same_name", "2"), (".External", "variable", "-1")]:
            self.native.append({"dll": "pkg", "dll_path": "synthetic/pkg.so", "interface": interface,
                                "name": name, "num_parameters": parameters,
                                "port_implementation_status": "unclassified", "port_behavior_status": "not_tested",
                                "port_safety_status": "not_assessed"})
        self.functions = [{column: "" for column in joiner.FUNCTION_COLUMNS}]
        self.functions[0].update(package="pkg", name="function", is_function="TRUE",
                                 port_implementation_status="unclassified", port_behavior_status="not_tested",
                                 port_safety_status="not_assessed")
        joiner.write_table(self.census / "native_routines.csv", joiner.NATIVE_COLUMNS, self.native)
        joiner.write_table(self.census / "functions.csv", joiner.FUNCTION_COLUMNS, self.functions)
        (self.census / "packages.csv").write_text("package,version\npkg,1\n")
        (self.census / "issues.csv").write_text("stage,item,severity,message\nnamespace,pkg,incomplete,synthetic issue\n")
        (self.census / "syntax.rds").write_bytes(b"synthetic binary evidence")
        self.census_metadata = {"schema_version": 1, "oracle_source_commit": manifest["source"]["commit"],
                               "oracle_manifest_sha256": joiner.manifest_digest(self.manifest_path),
                               "oracle_build_profile": manifest["build"],
                               "oracle_identity_validation": "installation_marker_and_runtime_version",
                               "oracle_runtime": "synthetic runtime; not executed", "selected_packages": ["pkg"],
                               "issues": 1, "census_exit_code": 1,
                               "oracle_runtime_sha256": "0" * 64, "census_script_sha256": "1" * 64,
                               "namespace_scan_complete": False, "gnu_api_inventory_complete": False,
                               "port_compatibility_assessed": False, "native_registrations": 4,
                               "function_bindings": 1,
                               "files_sha256": {path.name: joiner.digest(path) for path in self.census.iterdir()}}
        self.save_census()
        for name in ["Cargo.toml", "Cargo.lock", "crates/rmath/Cargo.toml", "crates/rmath/src/lib.rs",
                     "crates/rmath/build.rs"]:
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("synthetic compiled-source fixture; not compiled\n")
        self.artifact = self.root / "fake-binary"
        self.artifact.write_bytes(b"synthetic binary; never executed")
        (self.resolver / "probe.log").write_text("Synthetic unit fixture: no actual native resolver invoked.\n")
        self.rows = []
        for row in self.native:
            unsupported = row["interface"] == ".Fortran"
            self.rows.append({field: row[field] for field in joiner.RESOLVER_COLUMNS[:4]} |
                             {"resolver_status": "unsupported" if unsupported else "resolved",
                              "actual_interface": "" if unsupported else ".External2" if row["interface"] == ".External" else row["interface"],
                              "actual_arity_kind": "" if unsupported else "variadic" if row["num_parameters"] == "-1" else "fixed",
                              "actual_num_parameters": "" if unsupported else row["num_parameters"]})
        self.resolver_metadata = {"schema_version": 1, "oracle_source_commit": manifest["source"]["commit"],
                                  "oracle_manifest_sha256": joiner.manifest_digest(self.manifest_path),
                                  "source_revision": "a" * 40, "source_dirty": True,
                                  "source_files_sha256": {path.relative_to(self.source).as_posix(): joiner.digest(path)
                                                          for path in self.source.rglob("*") if path.is_file()},
                                  "compiled_artifact_sha256": joiner.digest(self.artifact),
                                  "rust_version": "synthetic rust compiler", "probe_command": ["synthetic-probe"],
                                  "probe_exit_code": 0, "execution_complete": True,
                                  "probe_log_sha256": joiner.digest(self.resolver / "probe.log"),
                                  "build_profile": {"flags": [], "default_features": True, "features": [],
                                                    "target": "synthetic-target", "profile": "test"}}
        self.save_resolver()

    def save_census(self) -> None:
        (self.census / "provenance.json").write_text(json.dumps(self.census_metadata))

    def save_resolver(self) -> None:
        joiner.write_table(self.resolver / "resolver.tsv", joiner.RESOLVER_COLUMNS, self.rows, "\t")
        self.resolver_metadata.update(census_native_sha256=joiner.digest(self.census / "native_routines.csv"),
                                      census_provenance_sha256=joiner.digest(self.census / "provenance.json"),
                                      resolver_tsv_sha256=joiner.digest(self.resolver / "resolver.tsv"))
        (self.resolver / "provenance.json").write_text(json.dumps(self.resolver_metadata))

    def run_join(self, output: Path | None = None, artifact: Path | None = None) -> dict:
        return joiner.join(self.census, self.resolver, output or self.output,
                           self.manifest_path, self.source, artifact)

    def rejected(self, pattern: str) -> None:
        with self.assertRaisesRegex(joiner.EvidenceError, pattern):
            self.run_join()
        self.assertFalse(self.output.exists(), "invalid evidence must not publish a partial report")

    def test_complete_join_retains_unsupported_and_unprobed_without_parity_claims(self) -> None:
        result = self.run_join(artifact=self.artifact)
        self.assertEqual((result["native_rows"], result["resolved_rows"], result["unsupported_rows"]), (4, 3, 1))
        self.assertTrue(result["evidence_join_complete"])
        self.assertFalse(result["full_gnu_r_parity"])
        self.assertFalse(result["namespace_scan_complete"])
        native = list(csv.DictReader(io.StringIO((self.output / "native_routines.csv").read_text())))
        self.assertEqual(len(native), 4)
        self.assertEqual(next(row for row in native if row["interface"] == ".External")["interface_status"], "external_family_only")
        self.assertTrue(all(row["port_behavior_status"] == "not_tested" for row in native))
        functions = list(csv.DictReader(io.StringIO((self.output / "functions.csv").read_text())))
        self.assertEqual(functions[0]["port_resolver_status"], "not_probed")
        self.assertEqual((self.output / "census_issues.csv").read_bytes(), (self.census / "issues.csv").read_bytes())

    def test_same_inputs_produce_identical_bytes_in_different_output_directories(self) -> None:
        self.run_join()
        second = self.root / "other-output"
        self.run_join(output=second)
        self.assertEqual({p.name: p.read_bytes() for p in self.output.iterdir()},
                         {p.name: p.read_bytes() for p in second.iterdir()})

    def test_legitimate_actual_interface_and_arity_mismatches_remain_explicit(self) -> None:
        row = next(row for row in self.rows if row["interface"] == ".Call")
        row.update(actual_interface=".C", actual_num_parameters="3")
        self.save_resolver()
        result = self.run_join()
        self.assertEqual(result["mismatched_rows"], 1)
        mismatches = list(csv.DictReader(io.StringIO((self.output / "resolver_mismatches.csv").read_text())))
        self.assertEqual((mismatches[0]["interface_status"], mismatches[0]["arity_status"]), ("mismatch", "mismatch"))
        self.assertFalse(result["behavior_assessed"])

    def test_same_symbol_in_different_dll_is_distinct_package_scoped_key(self) -> None:
        extra = dict(self.native[0], dll="other")
        self.native.append(extra)
        joiner.write_table(self.census / "native_routines.csv", joiner.NATIVE_COLUMNS, self.native)
        self.census_metadata["files_sha256"]["native_routines.csv"] = joiner.digest(self.census / "native_routines.csv")
        self.census_metadata["native_registrations"] = 5
        self.save_census()
        self.rows.append(dict(self.rows[0], dll="other"))
        self.save_resolver()
        self.assertEqual(self.run_join()["native_rows"], 5)

    def test_tampered_non_native_census_file_is_rejected(self) -> None:
        (self.census / "syntax.rds").write_bytes(b"tampered")
        self.rejected("census hash mismatch")

    def test_census_commit_and_manifest_binding_are_required(self) -> None:
        for field in ["oracle_source_commit", "oracle_manifest_sha256"]:
            with self.subTest(field=field):
                old = self.census_metadata[field]
                self.census_metadata[field] = "0" * len(old)
                self.save_census()
                self.rejected("census .* mismatch")
                self.census_metadata[field] = old
                self.save_census()

    def test_unrecorded_census_file_is_rejected(self) -> None:
        (self.census / "unrecorded").write_text("untracked evidence")
        self.rejected("unrecorded files")

    def test_census_path_escape_is_rejected(self) -> None:
        self.census_metadata["files_sha256"]["../outside"] = "0" * 64
        self.save_census()
        self.rejected("invalid census path")

    def test_duplicate_census_key_is_rejected_even_with_updated_hashes(self) -> None:
        self.native.append(dict(self.native[0]))
        joiner.write_table(self.census / "native_routines.csv", joiner.NATIVE_COLUMNS, self.native)
        self.census_metadata["files_sha256"]["native_routines.csv"] = joiner.digest(self.census / "native_routines.csv")
        self.save_census()
        self.rejected("duplicate census key")

    def test_missing_extra_and_duplicate_resolver_keys_are_rejected(self) -> None:
        original = copy.deepcopy(self.rows)
        for change, pattern in [(lambda: self.rows.pop(), "keys differ"),
                                (lambda: self.rows.append(dict(self.rows[0], name="extra")), "keys differ"),
                                (lambda: self.rows.append(dict(self.rows[0])), "duplicate resolver key")]:
            with self.subTest(pattern=pattern):
                self.rows = copy.deepcopy(original)
                change()
                self.save_resolver()
                self.rejected(pattern)

    def test_resolver_requested_arity_cannot_relabel_census(self) -> None:
        self.rows[0]["num_parameters"] = "9"
        self.save_resolver()
        self.rejected("requested arity differs")

    def test_fixed_variadic_metadata_must_be_self_consistent(self) -> None:
        self.rows[0]["actual_arity_kind"] = "variadic"
        self.save_resolver()
        self.rejected("inconsistent actual arity kind")

    def test_unsupported_rows_cannot_carry_resolved_metadata(self) -> None:
        self.rows[0]["resolver_status"] = "unsupported"
        self.save_resolver()
        self.rejected("unsupported row contains")

    def test_tampered_resolver_table_log_and_source_are_rejected(self) -> None:
        for path, pattern in [(self.resolver / "resolver.tsv", "resolver_tsv_sha256 mismatch"),
                              (self.resolver / "probe.log", "probe_log_sha256 mismatch"),
                              (self.source / "crates/rmath/src/lib.rs", "resolver source hash mismatch")]:
            with self.subTest(path=path):
                old = path.read_bytes()
                path.write_bytes(old + b"tamper")
                self.rejected(pattern)
                path.write_bytes(old)

    def test_source_manifest_requires_all_compiled_rmath_inputs(self) -> None:
        del self.resolver_metadata["source_files_sha256"]["crates/rmath/build.rs"]
        self.save_resolver()
        self.rejected("omits compiled")

    def test_probe_completion_and_compile_metadata_are_required(self) -> None:
        original = copy.deepcopy(self.resolver_metadata)
        for field, value, pattern in [("execution_complete", False, "did not complete"),
                                      ("probe_exit_code", True, "did not complete"),
                                      ("source_dirty", "false", "must be boolean"),
                                      ("build_profile", {}, "build_profile"),
                                      ("source_revision", "main", "must be a commit")]:
            with self.subTest(field=field):
                self.resolver_metadata = copy.deepcopy(original)
                self.resolver_metadata[field] = value
                self.save_resolver()
                self.rejected(pattern)

    def test_optional_retained_binary_must_match_recorded_digest(self) -> None:
        self.artifact.write_bytes(b"changed binary")
        with self.assertRaisesRegex(joiner.EvidenceError, "compiled artifact hash mismatch"):
            self.run_join(artifact=self.artifact)
        self.assertFalse(self.output.exists())

    def test_duplicate_json_fields_cannot_override_provenance(self) -> None:
        path = self.resolver / "provenance.json"
        original = path.read_text()
        path.write_text(original[:-1] + ', "execution_complete": false}')
        self.rejected("duplicate JSON key")

    def test_duplicate_headers_and_malformed_rows_are_rejected(self) -> None:
        path = self.resolver / "resolver.tsv"
        original = path.read_text()
        for altered in [original.replace("dll\tinterface", "dll\tdll", 1), original + "missing\tfields\n"]:
            with self.subTest(altered=altered[-30:]):
                path.write_text(altered)
                self.resolver_metadata["resolver_tsv_sha256"] = joiner.digest(path)
                (self.resolver / "provenance.json").write_text(json.dumps(self.resolver_metadata))
                self.rejected("columns must|malformed")

    def test_nonempty_output_is_not_overwritten_and_empty_output_is_accepted(self) -> None:
        self.output.mkdir()
        sentinel = self.output / "keep"
        sentinel.write_text("existing evidence")
        with self.assertRaisesRegex(joiner.EvidenceError, "new or empty"):
            self.run_join()
        self.assertEqual(sentinel.read_text(), "existing evidence")
        sentinel.unlink()
        self.assertEqual(self.run_join()["native_rows"], 4)

    def test_invalid_actual_interface_and_arity_are_not_classified_as_matches(self) -> None:
        original = copy.deepcopy(self.rows)
        for field, value, pattern in [("actual_interface", ".Bogus", "invalid actual interface"),
                                      ("actual_num_parameters", "-2", "invalid actual arity"),
                                      ("actual_num_parameters", "01", "invalid actual arity"),
                                      ("actual_num_parameters", "2147483648", "integer range")]:
            with self.subTest(field=field, value=value):
                self.rows = copy.deepcopy(original)
                self.rows[0][field] = value
                self.save_resolver()
                self.rejected(pattern)

    def test_census_count_cannot_claim_rows_not_present(self) -> None:
        self.census_metadata["native_registrations"] = 5
        self.save_census()
        self.rejected("native_registrations count mismatch")

    def test_cli_rejects_tampered_evidence_without_traceback_or_output(self) -> None:
        (self.census / "syntax.rds").write_bytes(b"changed")
        result = subprocess.run([sys.executable, str(ROOT / "scripts/join_gnu_r_api_evidence.py"),
                                 str(self.census), str(self.resolver), str(self.output),
                                 "--source-root", str(self.source)], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("ERROR: census hash mismatch", result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertFalse(self.output.exists())

    def test_unknown_metadata_cannot_smuggle_behavior_completion_claims(self) -> None:
        self.resolver_metadata["full_gnu_r_parity"] = True
        self.save_resolver()
        self.rejected("resolver provenance keys differ")

    def test_namespace_scan_cannot_hide_incomplete_issue_evidence(self) -> None:
        self.census_metadata["namespace_scan_complete"] = True
        self.save_census()
        self.rejected("completeness contradicts")


if __name__ == "__main__":
    unittest.main()
