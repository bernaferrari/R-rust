#!/usr/bin/env python3
"""Run fresh-session workflows against pinned GNU R and production Wasm."""

import argparse
import gzip
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib

from conformance_shards import file_digest, source_identity

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'tests/everyday-analysis/sessions'


def configure_environment():
    env = dict(os.environ)
    env.pop('LC_ALL', None)
    env.update({f'LC_{name}': 'C' for name in ('COLLATE', 'MONETARY', 'NUMERIC', 'TIME', 'MESSAGES')})
    env.update(LANG='C', LC_CTYPE='en_US.UTF-8', TZ='UTC',
               RPORT_CONFORMANCE_PROFILE='release', RPORT_RUNTIME_PACKAGE_POLICY='native')
    env['RUSTFLAGS'] = f"{env.get('RUSTFLAGS', '')} -Awarnings".strip()
    return env


def build_native_runner(directory, env):
    # Use the same exact Cargo artifact receipt and snapshot as the parity gate.
    script = r'''
set -euo pipefail
ROOT_DIR="$1"
source "$ROOT_DIR/scripts/conformance_artifacts.sh"
rlib="$(conformance_build_rmath --locked)"
cp -f "$rlib" "$2/librmath.rlib"
rustc --edition=2024 "$ROOT_DIR/tests/conformance/src/main.rs" \
    -L dependency="$(conformance_dependency_dir)" \
    --extern rmath="$2/librmath.rlib" -o "$2/native-r"
'''
    subprocess.run(['bash', '-c', script, 'build-native-runner', ROOT, directory],
                   cwd=ROOT, env=env, check=True)
    return [str(directory / 'native-r')]


def run_native_pair(command, directory, env, fixture):
    first = directory / 'first'
    second = directory / 'fresh'
    first.mkdir(parents=True)
    second.mkdir()
    shutil.copyfile(fixture / 'input.csv', first / 'input.csv')
    outputs = [subprocess.check_output([*command, fixture / 'import.R'], cwd=first,
                                      env=env, timeout=300, text=True)]
    shutil.copyfile(first / 'export.csv', second / 'export.csv')
    outputs.append(subprocess.check_output([*command, fixture / 'reload.R'], cwd=second,
                                         env=env, timeout=300, text=True))
    return {'outputs': outputs, 'export_sha256': file_digest(first / 'export.csv')}


def run_workspace(commands, directory, env, fixture, wasm_module):
    filename = 'analysis.RData'
    saved = {}
    for engine, command in commands.items():
        writer = directory / f'{engine}-writer'
        writer.mkdir()
        saved[engine] = subprocess.check_output(
            [*command, fixture / 'save.R'], cwd=writer, env=env, timeout=300, text=True)
        shutil.copyfile(writer / filename, directory / f'{engine}.RData')

    wasm = json.loads(subprocess.check_output(
        ['node', ROOT / 'tests/browser/everyday-workspace-restore.mjs',
         wasm_module, fixture, directory], cwd=ROOT, env=env, timeout=300, text=True))
    wasm_digest = wasm.pop('wasm_sha256')
    origins = ('gnu', 'native', 'wasm')
    artifacts = {}
    for origin in origins:
        workspace = directory / f'{origin}.RData'
        with gzip.open(workspace, 'rb') as stream:
            if stream.read(7) != b'RDX3\nX\n':
                raise ValueError(f'{origin} did not save a standard XDR version 3 workspace')
        artifacts[origin] = file_digest(workspace)

    outcomes = {'wasm': wasm}
    for engine, command in commands.items():
        restored = {}
        for origin in origins:
            reader = directory / f'{engine}-from-{origin}'
            reader.mkdir()
            shutil.copyfile(directory / f'{origin}.RData', reader / filename)
            restored[origin] = subprocess.check_output(
                [*command, fixture / 'reload.R'], cwd=reader, env=env, timeout=300, text=True)
        outcomes[engine] = dict(saved=saved[engine], restored=restored)
    expected = dict(saved=saved['gnu'],
                    restored=dict.fromkeys(origins, outcomes['gnu']['restored']['gnu']))
    return wasm_digest, expected, outcomes, artifacts


def run_pure_r_package(gnu, native, directory, env, fixture, wasm_module):
    package_root = ROOT / 'tests/real-packages'
    with (package_root / 'manifest.toml').open('rb') as source:
        package = next(row for row in tomllib.load(source)['package'] if row['name'] == 'R6')
    archive = package_root / package['tarball']
    digest = file_digest(archive)
    if digest != package['sha256'] or package['needs_compilation']:
        raise ValueError('expected the pinned pure-R R6 archive')
    gnu_library = directory / 'gnu-library'
    native_library = directory / 'native-library'
    gnu_library.mkdir()
    native_library.mkdir()
    r = Path(gnu[0]).resolve().with_name('R')
    subprocess.run([r, 'CMD', 'INSTALL', '--no-html', '-l', gnu_library, archive],
                   cwd=directory, env=env, stdout=sys.stderr, check=True, timeout=300)
    with tarfile.open(archive) as package_archive:
        package_archive.extractall(native_library, filter='data')

    def run(command, library):
        package_env = dict(env, R_LIBS=str(library), R_LIBS_USER='', R_LIBS_SITE='')
        return {'output': subprocess.check_output(
            [*command, fixture / 'analysis.R'], cwd=directory,
            env=package_env, timeout=300, text=True)}

    wasm = json.loads(subprocess.check_output(
        ['node', ROOT / 'tests/browser/everyday-pure-r-package.mjs', wasm_module],
        cwd=ROOT, env=env, timeout=300, text=True))
    wasm_sha256 = wasm.pop('wasm_sha256')
    if wasm.pop('archive_sha256') != digest:
        raise ValueError('Wasm used a different package archive')
    return wasm_sha256, run(gnu, gnu_library), dict(native=run(native, native_library), wasm=wasm), digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('wasm_package', type=Path)
    parser.add_argument('--workflow', choices=('file-round-trip', 'fresh-session-restore', 'pure-r-package'),
                        default='file-round-trip')
    args = parser.parse_args()
    rscript = shutil.which('Rscript')
    if not rscript:
        parser.error('put the pinned GNU Rscript on PATH')
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True):
        parser.error('run from a clean source checkout')
    env = configure_environment()
    subprocess.run([sys.executable, ROOT / 'scripts/validate_r_oracle.py', '--runtime', rscript],
                   cwd=ROOT, env=env, stdout=sys.stderr, check=True)
    source = source_identity(ROOT)
    fixture = FIXTURES / args.workflow
    wasm_module = args.wasm_package.resolve()
    with tempfile.TemporaryDirectory(prefix='rport-analysis-sessions.') as scratch:
        directory = Path(scratch)
        native_runner = build_native_runner(directory, env)
        gnu = [rscript, '--vanilla']
        artifacts = {}
        archive_digest = None
        if args.workflow == 'pure-r-package':
            wasm_sha256, expected, outcomes, archive_digest = run_pure_r_package(
                gnu, native_runner, directory, env, fixture, wasm_module)
        elif args.workflow == 'fresh-session-restore':
            wasm_sha256, expected, outcomes, artifacts = run_workspace(
                {'gnu': gnu, 'native': native_runner}, directory, env, fixture, wasm_module)
        else:
            expected = run_native_pair(gnu, directory / 'gnu', env, fixture)
            native = run_native_pair(native_runner, directory / 'native', env, fixture)
            wasm = json.loads(subprocess.check_output(
                ['node', ROOT / 'tests/browser/everyday-session-file.mjs', wasm_module, fixture],
                cwd=ROOT, env=env, timeout=300, text=True))
            wasm_sha256 = wasm.pop('wasm_sha256')
            outcomes = dict(native=native, wasm=wasm)
        if wasm_sha256 != file_digest(wasm_module / 'r_wasm_bg.wasm'):
            parser.error('the Wasm report does not match the production module')
        report = dict(schema_version=1, **source, wasm_sha256=wasm_sha256,
                      oracle_manifest_sha256=file_digest(ROOT / 'oracle/r-oracle.json'),
                      workflow=args.workflow,
                      fixture_sha256={p.name: file_digest(p) for p in sorted(fixture.iterdir())},
                      expected=expected, **outcomes)
        if artifacts:
            report['workspace_sha256'] = artifacts
        if archive_digest:
            report['archive_sha256'] = archive_digest
        print(json.dumps(report, indent=2))
        return int(any(outcome != expected for outcome in outcomes.values()))


if __name__ == '__main__':
    sys.exit(main())
