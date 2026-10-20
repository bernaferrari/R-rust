#!/usr/bin/env python3
"""Copy only the R library files already embedded by Rust, from verified Git blobs."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
COMMIT = 'd4cc5d9e196a144bbb087a798bb945b37121383b'
REPOSITORY = 'https://github.com/wch/r-source.git'
INCLUDE = re.compile(r'include_(?:str|bytes)!\s*\(\s*"([^"]+)"\s*\)')


def selected_sources(root):
    asset_root = root / 'crates/rmath/upstream'
    selected = set()
    for rust in (root / 'crates').rglob('*.rs'):
        for name in INCLUDE.findall(rust.read_text()):
            if 'r-source/src/library/' in name:
                selected.add(name.split('r-source/src/library/', 1)[1])
            elif '/upstream/' in name:
                resolved = (rust.parent / name).resolve()
                if resolved.is_relative_to(asset_root.resolve()):
                    selected.add(resolved.relative_to(asset_root.resolve()).as_posix())
    if not selected or any(Path(name).suffix != '.R' or '..' in Path(name).parts for name in selected):
        raise ValueError('expected a nonempty set of packaged .R inputs')
    return sorted(selected)


def vendor(root, source):
    head = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if head != COMMIT:
        raise ValueError(f'upstream checkout must remain at {COMMIT}, found {head}')
    files = {}
    blobs = {}
    for name in selected_sources(root):
        upstream = 'src/library/' + name
        expected = subprocess.check_output(['git', '-C', str(source), 'show', f'{COMMIT}:{upstream}'])
        if (source / upstream).read_bytes() != expected:
            raise ValueError('upstream working file differs from pinned Git blob: ' + upstream)
        blobs[name] = expected
        files[name] = {'source_path': upstream, 'bytes': len(expected),
                       'sha256': hashlib.sha256(expected).hexdigest()}
    license_bytes = subprocess.check_output(['git', '-C', str(source), 'show', f'{COMMIT}:COPYING'])
    destination = root / 'crates/rmath/upstream'
    destination.mkdir(parents=True, exist_ok=True)
    for name, data in blobs.items():
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    (destination / 'COPYING').write_bytes(license_bytes)
    manifest = {'schema_version': 1,
                'source': {'repository': REPOSITORY, 'commit': COMMIT},
                'generator': 'scripts/vendor_embedded_r_sources.py',
                'license': {'spdx': 'GPL-2.0-or-later', 'path': 'COPYING',
                            'sha256': hashlib.sha256(license_bytes).hexdigest()},
                'files': files}
    (destination / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-tree', type=Path, default=ROOT / 'r-source')
    args = parser.parse_args()
    result = vendor(ROOT, args.source_tree.resolve())
    print(f"Vendored {len(result['files'])} exact GNU R assets; {sum(f['bytes'] for f in result['files'].values())} bytes")


if __name__ == '__main__':
    main()
