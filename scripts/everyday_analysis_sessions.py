#!/usr/bin/env python3
"""Run the fresh-session file workflow against pinned GNU R and production Wasm."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from conformance_shards import file_digest, source_identity

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'tests/everyday-analysis/sessions/file-round-trip'


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


def run_native_pair(command, directory, env):
    first = directory / 'first'
    second = directory / 'fresh'
    first.mkdir(parents=True)
    second.mkdir()
    shutil.copyfile(FIXTURE / 'input.csv', first / 'input.csv')
    outputs = [subprocess.check_output([*command, FIXTURE / 'import.R'], cwd=first,
                                      env=env, timeout=300, text=True)]
    shutil.copyfile(first / 'export.csv', second / 'export.csv')
    outputs.append(subprocess.check_output([*command, FIXTURE / 'reload.R'], cwd=second,
                                         env=env, timeout=300, text=True))
    return {'outputs': outputs, 'export_sha256': file_digest(first / 'export.csv')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('wasm_package', type=Path)
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
    with tempfile.TemporaryDirectory(prefix='rport-analysis-sessions.') as scratch:
        directory = Path(scratch)
        native_runner = build_native_runner(directory, env)
        expected = run_native_pair([rscript, '--vanilla'], directory / 'gnu', env)
        native = run_native_pair(native_runner, directory / 'native', env)
        wasm_module = args.wasm_package.resolve()
        wasm = json.loads(subprocess.check_output(
            ['node', ROOT / 'tests/browser/everyday-session-file.mjs', wasm_module, FIXTURE],
            cwd=ROOT, env=env, timeout=300, text=True))
        wasm_sha256 = wasm.pop('wasm_sha256')
        if wasm_sha256 != file_digest(wasm_module / 'r_wasm_bg.wasm'):
            parser.error('the Wasm report does not match the production module')
        report = dict(schema_version=1, **source, wasm_sha256=wasm_sha256,
                      oracle_manifest_sha256=file_digest(ROOT / 'oracle/r-oracle.json'),
                      workflow='file-round-trip',
                      fixture_sha256={p.name: file_digest(p) for p in sorted(FIXTURE.iterdir())},
                      expected=expected, native=native, wasm=wasm)
        print(json.dumps(report, indent=2))
        return int(native != expected or wasm != expected)


if __name__ == '__main__':
    sys.exit(main())
