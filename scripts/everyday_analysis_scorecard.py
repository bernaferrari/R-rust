#!/usr/bin/env python3
"""Score the workflow inventory from exact GNU and production Wasm reports."""

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path

if __package__:
    from .conformance_shards import file_digest, rows_from_report
else:
    from conformance_shards import file_digest, rows_from_report

ROOT = Path(__file__).resolve().parents[1]


def native_results(paths):
    provenance = None
    cases = {}
    for path in paths:
        report = json.loads(path.read_text())
        policy = dict(report['execution'])
        policy.pop('shard', None)
        if not (policy['strict'] and policy['pinned_oracle_required']
                and policy['profile'] == 'release' and report['execution_complete']):
            raise ValueError(f'{path}: expected a complete pinned strict release run or shard')
        if provenance is not None and provenance != policy:
            raise ValueError('native reports differ in source, corpus, oracle or policy')
        provenance = policy
        rows = report['cases'] if 'cases' in report else rows_from_report(report)
        for row in rows:
            if row['kind'] != 'normal':
                continue
            if row['case'] in cases:
                raise ValueError(f'duplicate native case: {row["case"]}')
            cases[row['case']] = {'pass': 'pass', 'skip': 'unmeasured'}.get(row['status'], 'fail')
    return provenance, cases


def wasm_results(log, module):
    digest = hashlib.sha256(module.read_bytes()).hexdigest()
    rows = [json.loads(line) for line in log.read_text().splitlines() if line.startswith('{')]
    if [row['wasm_sha256'] for row in rows if 'wasm_sha256' in row] != [digest]:
        raise ValueError('Wasm report must refer to exactly the supplied module')
    complete = {row['generation'] for row in rows if 'contracts_passed' in row}
    results = defaultdict(dict)
    for row in rows:
        if row.get('phase') != 'public-contract':
            continue
        source = row['source'].replace('\\', '/')
        if '/tests/conformance/cases/' not in source:
            continue
        case = Path(source).stem
        generation = row['generation']
        if generation in results[case]:
            raise ValueError(f'duplicate Wasm case and generation: {case}, {generation}')
        results[case][generation] = row['status']

    cases = {}
    for case, generations in results.items():
        if 'fail' in generations.values():
            cases[case] = 'fail'
        elif {0, 1} <= complete and all(generations.get(n) == 'pass' for n in (0, 1)):
            cases[case] = 'pass'
    return digest, cases


def session_workflow_results(path, native_policy, wasm_digest):
    report = json.loads(path.read_text())
    workflow = report['workflow']
    provenance = ('source_commit', 'source_sha256', 'oracle_manifest_sha256')
    if (report['schema_version'] != 1 or workflow not in ('file-round-trip', 'fresh-session-restore')
            or any(report[name] != native_policy[name] for name in provenance)
            or report['wasm_sha256'] != wasm_digest):
        raise ValueError('session report differs in workflow, source, oracle or Wasm artifact')
    fixture = ROOT / 'tests/everyday-analysis/sessions' / workflow
    if report['fixture_sha256'] != {p.name: file_digest(p) for p in sorted(fixture.iterdir())}:
        raise ValueError('session report differs in workflow fixtures')
    expected = report['expected']
    if workflow == 'file-round-trip':
        if (len(expected['outputs']) != 2 or not all(expected['outputs'])
                or not is_sha256(expected['export_sha256'])):
            raise ValueError('session report must contain both GNU outputs and the exported file digest')
    else:
        engines = {'gnu', 'native', 'wasm'}
        artifacts = report['workspace_sha256']
        if (set(artifacts) != engines or not all(map(is_sha256, artifacts.values()))
                or set(expected['restored']) != engines
                or not expected['saved'] or not all(expected['restored'].values())
                or report['gnu'] != expected):
            raise ValueError('workspace report must prove each origin restores in pinned GNU R')
    return workflow, {
        side: 'pass' if report[side] == expected else 'fail' for side in ('native', 'wasm')
    }


def is_sha256(value):
    return isinstance(value, str) and len(value) == 64 and all(c in '0123456789abcdef' for c in value)


def score(workflows, native, wasm, sessions=None):
    if not workflows or len({row['id'] for row in workflows}) != len(workflows):
        raise ValueError('expected a nonempty workflow inventory with distinct IDs')
    mapped = [row['case'] for row in workflows if 'case' in row]
    if len(set(mapped)) != len(mapped):
        raise ValueError('a fixture cannot count for more than one workflow')
    sessions = sessions or {}
    rows = [dict(row, **sessions.get(row['id'], {
        'native': native.get(row.get('case'), 'unmeasured'),
        'wasm': wasm.get(row.get('case'), 'unmeasured'),
    })) for row in workflows]
    totals = {}
    for platform in ('native', 'wasm'):
        counts = Counter(row[platform] for row in rows)
        totals[platform] = {status: counts[status] for status in ('pass', 'fail', 'unmeasured')}
        totals[platform]['percent_verified'] = round(100 * counts['pass'] / len(rows), 1)
    return rows, totals


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native-report', action='append', type=Path, required=True)
    parser.add_argument('--wasm-log', type=Path, required=True)
    parser.add_argument('--wasm-module', type=Path, required=True)
    parser.add_argument('--session-report', action='append', type=Path, default=[])
    args = parser.parse_args()
    try:
        inventory = json.loads((ROOT / 'tests/everyday-analysis/workflows.json').read_text())
        native_policy, native = native_results(args.native_report)
        wasm_sha256, wasm = wasm_results(args.wasm_log, args.wasm_module)
        sessions = {}
        for path in args.session_report:
            workflow, outcome = session_workflow_results(path, native_policy, wasm_sha256)
            if workflow in sessions:
                raise ValueError(f'duplicate session report for {workflow}')
            sessions[workflow] = outcome
        workflows, totals = score(inventory['workflows'], native, wasm, sessions)
    except (KeyError, OSError, ValueError) as err:
        parser.error(str(err))
    print(json.dumps({'inventory_version': inventory['version'], 'workflow_count': len(workflows),
                      'native_execution': native_policy, 'wasm_sha256': wasm_sha256,
                      'totals': totals, 'workflows': workflows}, indent=2))


if __name__ == '__main__':
    main()
