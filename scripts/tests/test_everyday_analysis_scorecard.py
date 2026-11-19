import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from scripts import everyday_analysis_scorecard as scorecard


class EverydayScorecardTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)

    def test_unmeasured_and_failing_workflows_stay_in_denominator(self):
        workflows = [{'id': 'a', 'case': '001_a'}, {'id': 'b', 'case': '002_b'}, {'id': 'c'}]
        results, totals = scorecard.score(workflows, {'001_a': 'pass', '002_b': 'fail'}, {})
        self.assertEqual(totals['native'], {'pass': 1, 'fail': 1, 'unmeasured': 1,
                                            'percent_verified': 33.3})
        self.assertEqual(totals['wasm']['unmeasured'], 3)
        self.assertEqual(results[1]['native'], 'fail')

    def test_wasm_pass_requires_both_completed_recovery_runs(self):
        module = self.directory / 'module.wasm'
        module.write_bytes(b'module payload')
        digest = hashlib.sha256(module.read_bytes()).hexdigest()
        path = self.directory / 'wasm.log'
        rows = [{'wasm_sha256': digest}]
        for generation in (0, 1):
            rows.append({'phase': 'public-contract', 'generation': generation, 'status': 'pass',
                         'source': '/repo/tests/conformance/cases/001_a.R'})
        rows.append({'generation': 0, 'contracts_passed': 1})
        path.write_text('\n'.join(map(json.dumps, rows)))
        self.assertEqual(scorecard.wasm_results(path, module), (digest, {}))
        rows.append({'generation': 1, 'contracts_passed': 1})
        path.write_text('\n'.join(map(json.dumps, rows)))
        self.assertEqual(scorecard.wasm_results(path, module), (digest, {'001_a': 'pass'}))
        module.write_bytes(b'different module')
        with self.assertRaisesRegex(ValueError, 'refer to exactly the supplied module'):
            scorecard.wasm_results(path, module)

    def test_native_reports_cannot_mix_source_commits(self):
        policy = {'strict': True, 'pinned_oracle_required': True, 'profile': 'release'}
        paths = [self.directory / 'first.json', self.directory / 'second.json']
        for commit, path in enumerate(paths):
            report = {'execution': dict(policy, source_commit=str(commit)),
                      'execution_complete': True, 'cases': []}
            path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'native reports differ'):
            scorecard.native_results(paths)

    def test_fresh_session_report_requires_matching_artifact_and_export(self):
        provenance = {'source_commit': 'commit', 'source_sha256': 'runtime',
                      'oracle_manifest_sha256': 'oracle'}
        fixture = scorecard.ROOT / 'tests/everyday-analysis/sessions/file-round-trip'
        fixture_hashes = {p.name: scorecard.file_digest(p) for p in sorted(fixture.iterdir())}
        expected = {'outputs': ['wrote the file\n', 'read the file\n'],
                    'export_sha256': 'a' * 64}
        changed = dict(expected, export_sha256='b' * 64)
        path = self.directory / 'sessions.json'
        path.write_text(json.dumps(dict(
            schema_version=1, **provenance, workflow='file-round-trip',
            wasm_sha256='module', fixture_sha256=fixture_hashes,
            expected=expected, native=expected, wasm=changed,
        )))
        self.assertEqual(scorecard.file_round_trip_results(path, provenance, 'module'),
                         {'native': 'pass', 'wasm': 'fail'})
        with self.assertRaisesRegex(ValueError, 'session report differs'):
            scorecard.file_round_trip_results(path, provenance, 'other-module')


if __name__ == '__main__':
    unittest.main()
