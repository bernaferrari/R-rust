#!/usr/bin/env python3
"""Run fresh-session workflows against pinned GNU R and production Wasm."""

import argparse
import csv
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
from everyday_pdf import EXPECTED_SCATTERPLOT, inspect_scatterplot
from everyday_report import EXPECTED_FIGURE, FORECASTS, OUTPUTS, PHASES, inspect_figure

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


def build_native_runner(directory, env, renderplot=False):
    # Use the same exact Cargo artifact receipt and snapshot as the parity gate.
    script = r'''
set -euo pipefail
ROOT_DIR="$1"
source "$ROOT_DIR/scripts/conformance_artifacts.sh"
build_args=(--locked)
if [[ "$3" == renderplot ]]; then
    build_args+=(--features renderplot-device)
fi
rlib="$(conformance_build_rmath "${build_args[@]}")"
cp -f "$rlib" "$2/librmath.rlib"
rustc --edition=2024 "$ROOT_DIR/tests/conformance/src/main.rs" \
    -L dependency="$(conformance_dependency_dir)" \
    --extern rmath="$2/librmath.rlib" -o "$2/native-r"
'''
    subprocess.run(['bash', '-c', script, 'build-native-runner', ROOT, directory,
                    'renderplot' if renderplot else 'base'], cwd=ROOT, env=env, check=True)
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


def run_declared_package_dependencies(gnu, native, directory, env, fixture, wasm_module, archive):
    source = directory / 'dependency-source'
    installed = directory / 'dependency-installed'
    libraries = ('pinned', 'synthetic', 'wrappers')
    for tree in (source, installed):
        for library in libraries:
            (tree / library).mkdir(parents=True)
    with tarfile.open(archive) as package_archive:
        package_archive.extractall(source / 'pinned', filter='data')
    pinned = source / 'pinned/R6'
    synthetic = source / 'synthetic/R6'
    shutil.copytree(pinned, synthetic)
    description = synthetic / 'DESCRIPTION'
    version = description.read_text()
    if 'Version: 2.6.1\n' not in version:
        raise ValueError('unexpected version in pinned R6 source')
    description.write_text(version.replace('Version: 2.6.1\n', 'Version: 3.1.0\n', 1))

    r = Path(gnu[0]).resolve().with_name('R')
    install_env = dict(env, R_LIBS=str(installed / 'synthetic'), R_LIBS_USER='', R_LIBS_SITE='')

    def install(package, library):
        subprocess.run([r, 'CMD', 'INSTALL', '--no-html', '-l', installed / library, package],
                       cwd=directory, env=install_env, stdout=sys.stderr, check=True, timeout=300)

    install(pinned, 'pinned')
    # The version-labeled copy allows GNU to install the negative fixtures.
    # At runtime the negative tests resolve the authentic pinned release.
    install(synthetic, 'synthetic')
    for name, dependency, imports in (
        ('NeedsR6', 'Imports: R6 (>= 2.6.1)', True),
        ('TooNewImports', 'Imports: R6 (>= 3.0.0)', True),
        ('UnusedImports', 'Imports: R6 (>= 3.0.0)', False),
        ('TooNewDepends', 'Depends: R6 (>= 3.0.0)', False),
        ('NeedsNewR', 'Depends: R (>= 4.0.0)', False),
    ):
        package = source / 'wrappers' / name
        (package / 'R').mkdir(parents=True)
        (package / 'DESCRIPTION').write_text(
            f'Package: {name}\nVersion: 1.0.0\nTitle: Dependency Version Check\n'
            'Description: A fixture for checking package version requirements.\n'
            'Maintainer: Test Fixture <test@example.invalid>\n'
            f'Author: Test Fixture\nLicense: GPL-2\n{dependency}\n')
        import_line = 'importFrom(R6, R6Class)\n' if imports else ''
        (package / 'NAMESPACE').write_text(f'export(result)\n{import_line}')
        body = 'R6Class("Result", public = list(value = 7L))$new()$value' if imports else '7L'
        (package / 'R/result.R').write_text(f'result <- function() {body}\n')
        install(package, 'wrappers')

    # Stage the newer R requirement after installation, as a package built
    # for a future R release cannot be installed by the pinned oracle.
    description = source / 'wrappers/NeedsNewR/DESCRIPTION'
    description.write_text(description.read_text().replace('R (>= 4.0.0)', 'R (>= 9999.0.0)'))
    subprocess.run([*gnu, '-e',
                    'path <- commandArgs(TRUE)[1L]; info <- readRDS(path); '
                    'info$Rdepends2[[1L]]$version <- numeric_version("9999.0.0"); saveRDS(info, path)',
                    installed / 'wrappers/NeedsNewR/Meta/package.rds'],
                   env=install_env, check=True, timeout=300)

    run_env = dict(env, R_LIBS='', R_LIBS_USER='', R_LIBS_SITE='')
    outputs = {
        engine: subprocess.check_output([*runner, fixture / 'dependencies.R'], cwd=working,
                                        env=run_env, timeout=300, text=True)
        for engine, runner, working in (('gnu', gnu, installed), ('native', native, source))
    }
    wasm = json.loads(subprocess.check_output(
        ['node', ROOT / 'tests/browser/everyday-package-dependencies.mjs', wasm_module, source],
        env=run_env, timeout=300, text=True))
    return outputs, wasm


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
    outputs, dependency_wasm = run_declared_package_dependencies(
        gnu, native, directory, env, fixture, wasm_module, archive)
    if dependency_wasm.pop('wasm_sha256') != wasm_sha256:
        raise ValueError('package dependency checks used a different Wasm module')
    expected = dict(run(gnu, gnu_library), dependency_output=outputs['gnu'])
    native_result = dict(run(native, native_library), dependency_output=outputs['native'])
    return wasm_sha256, expected, dict(native=native_result, wasm=wasm | dependency_wasm), digest


def run_plot_export(gnu, native, directory, env, fixture, wasm_module):
    outputs = {}
    evidence = {}
    wasm_dir = directory / 'wasm'
    wasm_dir.mkdir()
    wasm = json.loads(subprocess.check_output(
        ['node', ROOT / 'tests/browser/everyday-pdf.mjs', wasm_module, fixture, wasm_dir],
        cwd=ROOT, env=env, timeout=300, text=True))
    for side, command in {'gnu': gnu, 'native': native, 'wasm': None}.items():
        work = directory / side
        if command is not None:
            work.mkdir()
        pdf = work / 'weekly-observations.pdf'
        output = wasm['output'] if command is None else subprocess.check_output(
            [*command, fixture / 'plot.R'], cwd=work, env=env, timeout=300, text=True)
        manifest, counts = inspect_scatterplot(pdf)
        outputs[side] = dict(output=output, **manifest)
        evidence[side] = dict(pdf_sha256=file_digest(pdf), pdf_bytes=pdf.stat().st_size,
                              pixel_counts=counts)
    if any(outputs['gnu'][key] != value for key, value in EXPECTED_SCATTERPLOT.items()):
        raise ValueError('the pinned GNU plot does not satisfy the independent artifact checks')
    native_dir = directory / 'native'
    target = native_dir / 'protected.pdf'
    shutil.copyfile(native_dir / 'weekly-observations.pdf', target)
    before = file_digest(target)
    safety = subprocess.check_output([*native, fixture / 'safety.R'], cwd=native_dir,
                                     env=env, timeout=300, text=True)
    evidence['native']['preserved_sha256'] = file_digest(target)
    evidence['wasm']['preserved_sha256'] = wasm['preserved_sha256']
    if (evidence['native']['preserved_sha256'] != before
            or evidence['wasm']['preserved_sha256'] != evidence['wasm']['pdf_sha256']
            or safety != wasm['safety']):
        raise ValueError('a failed PDF export did not preserve the existing file')
    if safety != 'Retained the previous file after refusing unsupported PDF content\n':
        raise ValueError('missing PDF failure and recovery evidence')
    return wasm['wasm_sha256'], outputs.pop('gnu'), outputs, evidence


def run_repeatable_report(gnu, native, directory, env, fixture, wasm_module):
    wasm_dir = directory / 'wasm'
    wasm_dir.mkdir()
    wasm = json.loads(subprocess.check_output(
        ['node', ROOT / 'tests/browser/everyday-report.mjs', wasm_module, fixture, wasm_dir],
        cwd=ROOT, env=env, timeout=300, text=True))
    outcomes, evidence = {}, {}
    for side, command in {'gnu': gnu, 'native': native, 'wasm': None}.items():
        outcomes[side], evidence[side] = {}, {}
        for phase in PHASES:
            work = directory / side / phase
            if command is None:
                output = wasm['stdout'][phase]
            else:
                work.mkdir(parents=True)
                shutil.copyfile(fixture / 'report.R', work / 'report.R')
                shutil.copyfile(fixture / (phase + '.csv'), work / 'observations.csv')
                if phase == 'rebuild':
                    for name in OUTPUTS:
                        shutil.copyfile(work.parent / 'initial' / name, work / name)
                output = subprocess.check_output(
                    [*command, fixture / 'run.R'], cwd=work, env=env, timeout=300, text=True)
            with (work / 'forecast.csv').open(newline='') as stream:
                rows = list(csv.reader(stream))
            if (any(len(row) != 2 for row in rows) or not rows
                    or rows[0] != ['Scenario', 'Estimate']
                    or [row[0] for row in rows[1:]] != ['Standard', 'Irrigated']):
                raise ValueError(f'{side} {phase} did not produce the forecast table')
            figure, raster_sha256 = inspect_figure(work / 'field-trial.pdf', phase)
            html, table, pdf = (file_digest(work / name) for name in OUTPUTS)
            outcomes[side][phase] = dict(output=output, forecasts=[row[1] for row in rows[1:]],
                                         html_sha256=html, table_sha256=table, figure=figure)
            evidence[side][phase] = dict(pdf_sha256=pdf, raster_sha256=raster_sha256,
                                         pdf_bytes=(work / 'field-trial.pdf').stat().st_size)
        if any(outcomes[side]['initial'][field] == outcomes[side]['rebuild'][field]
               for field in ('html_sha256', 'table_sha256')) or (
            evidence[side]['initial']['raster_sha256'] == evidence[side]['rebuild']['raster_sha256']
        ):
            raise ValueError(f'{side} retained stale report output after the input changed')
    expected = outcomes.pop('gnu')
    if any(expected[phase]['forecasts'] != FORECASTS[phase]
           or expected[phase]['figure'] != EXPECTED_FIGURE for phase in PHASES):
        raise ValueError('pinned GNU report did not satisfy the independent forecast and figure checks')
    return wasm['wasm_sha256'], expected, outcomes, evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('wasm_package', type=Path)
    parser.add_argument('--workflow', choices=('file-round-trip', 'fresh-session-restore', 'pure-r-package',
                                               'base-plot-export', 'repeatable-report'),
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
        native_runner = build_native_runner(
            directory, env, renderplot=args.workflow in ('base-plot-export', 'repeatable-report'))
        gnu = [rscript, '--vanilla']
        artifacts = {}
        archive_digest = None
        pdf_evidence = None
        report_evidence = None
        if args.workflow == 'base-plot-export':
            wasm_sha256, expected, outcomes, pdf_evidence = run_plot_export(
                gnu, native_runner, directory, env, fixture, wasm_module)
        elif args.workflow == 'repeatable-report':
            wasm_sha256, expected, outcomes, report_evidence = run_repeatable_report(
                gnu, native_runner, directory, env, fixture, wasm_module)
        elif args.workflow == 'pure-r-package':
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
        if pdf_evidence:
            report['pdf_evidence'] = pdf_evidence
        if report_evidence:
            report['report_evidence'] = report_evidence
        print(json.dumps(report, indent=2))
        return int(any(outcome != expected for outcome in outcomes.values()))


if __name__ == '__main__':
    sys.exit(main())
