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
        self.assertEqual(scorecard.session_workflow_results(path, provenance, 'module'),
                         ('file-round-trip', {'native': 'pass', 'wasm': 'fail'}))
        with self.assertRaisesRegex(ValueError, 'session report differs'):
            scorecard.session_workflow_results(path, provenance, 'other-module')

    def test_workspace_requires_gnu_to_restore_all_produced_files(self):
        provenance = {'source_commit': 'commit', 'source_sha256': 'runtime',
                      'oracle_manifest_sha256': 'oracle'}
        fixture = scorecard.ROOT / 'tests/everyday-analysis/sessions/fresh-session-restore'
        fixture_hashes = {p.name: scorecard.file_digest(p) for p in sorted(fixture.iterdir())}
        origins = ('gnu', 'native', 'wasm')
        expected = {'saved': 'saved\n', 'restored': dict.fromkeys(origins, 'restored\n')}
        report = dict(schema_version=1, **provenance, workflow='fresh-session-restore',
                      wasm_sha256='module', fixture_sha256=fixture_hashes,
                      workspace_sha256=dict.fromkeys(origins, 'a' * 64),
                      expected=expected, native=expected, wasm=expected, gnu=expected)
        path = self.directory / 'workspace.json'
        path.write_text(json.dumps(report))
        self.assertEqual(scorecard.session_workflow_results(path, provenance, 'module'),
                         ('fresh-session-restore', {'native': 'pass', 'wasm': 'pass'}))
        report['gnu'] = dict(saved=expected['saved'], restored={'gnu': 'restored\n'})
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'each origin restores in pinned GNU R'):
            scorecard.session_workflow_results(path, provenance, 'module')

    def test_pure_r_package_requires_the_pinned_archive(self):
        provenance = {'source_commit': 'commit', 'source_sha256': 'runtime',
                      'oracle_manifest_sha256': 'oracle'}
        fixture = scorecard.ROOT / 'tests/everyday-analysis/sessions/pure-r-package'
        archive = scorecard.ROOT / 'tests/real-packages/vendor/R6_2.6.1.tar.gz'
        expected = {'output': 'Loaded R6 and computed a mean\n'}
        report = dict(schema_version=1, **provenance, workflow='pure-r-package',
                      wasm_sha256='module', archive_sha256=scorecard.file_digest(archive),
                      fixture_sha256={'analysis.R': scorecard.file_digest(fixture / 'analysis.R')},
                      expected=expected, native=expected, wasm=expected)
        path = self.directory / 'package.json'
        path.write_text(json.dumps(report))
        self.assertEqual(scorecard.session_workflow_results(path, provenance, 'module'),
                         ('pure-r-package', {'native': 'pass', 'wasm': 'pass'}))
        report['archive_sha256'] = '0' * 64
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'pinned archive digest'):
            scorecard.session_workflow_results(path, provenance, 'module')

    def test_rebuilt_report_requires_updated_visible_figure(self):
        provenance = {'source_commit': 'commit', 'source_sha256': 'runtime',
                      'oracle_manifest_sha256': 'oracle'}
        fixture = scorecard.ROOT / 'tests/everyday-analysis/sessions/repeatable-report'
        fixture_hashes = {p.name: scorecard.file_digest(p) for p in sorted(fixture.iterdir())}
        expected = {}
        figures = {}
        for phase, digest in (('initial', 'a' * 64), ('rebuild', 'b' * 64)):
            expected[phase] = dict(forecasts=scorecard.FORECASTS[phase], figure=scorecard.EXPECTED_FIGURE,
                                   html_sha256=digest, table_sha256=digest, output='')
            figures[phase] = dict(pdf_sha256=digest, raster_sha256=digest, pdf_bytes=1234)
        evidence = {side: {phase: dict(row) for phase, row in figures.items()}
                    for side in ('gnu', 'native', 'wasm')}
        report = dict(schema_version=1, **provenance, workflow='repeatable-report',
                      wasm_sha256='module', fixture_sha256=fixture_hashes, report_evidence=evidence,
                      expected=expected, native=expected, wasm=expected)
        path = self.directory / 'report.json'
        path.write_text(json.dumps(report))
        self.assertEqual(scorecard.session_workflow_results(path, provenance, 'module'),
                         ('repeatable-report', {'native': 'pass', 'wasm': 'pass'}))
        evidence['wasm']['rebuild']['raster_sha256'] = evidence['wasm']['initial']['raster_sha256']
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'requires both forecasts and a changed rendered figure'):
            scorecard.session_workflow_results(path, provenance, 'module')


if __name__ == '__main__':
    unittest.main()
