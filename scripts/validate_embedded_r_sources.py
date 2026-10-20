#!/usr/bin/env python3
"""Authenticate committed R library inputs and their actual Rust include paths."""
import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
COMMIT = 'd4cc5d9e196a144bbb087a798bb945b37121383b'
REPOSITORY = 'https://github.com/wch/r-source.git'
INCLUDE = re.compile(r'include_(?:str|bytes)!\s*\(\s*"([^"]+)"\s*\)')


def validate(root):
    assets = root / 'crates/rmath/upstream'
    if assets.is_symlink():
        raise ValueError('asset directory must be committed storage')
    manifest = json.loads((assets / 'manifest.json').read_text())
    if manifest.get('schema_version') != 1 or manifest.get('source') != {'repository': REPOSITORY, 'commit': COMMIT}:
        raise ValueError('unexpected embedded R provenance')
    files = manifest.get('files')
    if not isinstance(files, dict) or not files:
        raise ValueError('embedded R inventory is empty')
    for name, row in files.items():
        path = Path(name)
        if path.is_absolute() or '..' in path.parts or path.suffix != '.R' or not path.parts or path.parts[0] != 'base':
            raise ValueError('invalid packaged R path: ' + name)
        target = assets / path
        if target.is_symlink() or not target.resolve().is_relative_to(assets.resolve()):
            raise ValueError('packaged R path escapes asset storage: ' + name)
        if row.get('source_path') != 'src/library/' + name:
            raise ValueError('unexpected upstream R path: ' + name)
        data = target.read_bytes()
        if len(data) != row.get('bytes') or hashlib.sha256(data).hexdigest() != row.get('sha256'):
            raise ValueError('embedded R content differs from pinned manifest: ' + name)
    license_row = manifest.get('license', {})
    if license_row.get('spdx') != 'GPL-2.0-or-later' or license_row.get('path') != 'COPYING':
        raise ValueError('missing GNU R license provenance')
    if hashlib.sha256((assets / 'COPYING').read_bytes()).hexdigest() != license_row.get('sha256'):
        raise ValueError('GNU R license content changed')
    expected = set(files) | {'COPYING', 'manifest.json'}
    actual = {p.relative_to(assets).as_posix() for p in assets.rglob('*') if p.is_file()}
    if actual != expected:
        raise ValueError('asset files differ from pinned inventory')
    references = set()
    for rust in (root / 'crates').rglob('*.rs'):
        for name in INCLUDE.findall(rust.read_text()):
            if 'r-source/' in name:
                raise ValueError('compile-time input still depends on ignored r-source: ' + str(rust.relative_to(root)))
            resolved = (rust.parent / name).resolve()
            if resolved.is_relative_to(assets.resolve()):
                references.add(resolved.relative_to(assets.resolve()).as_posix())
    if references != set(files):
        raise ValueError('Rust include paths differ from pinned asset inventory')
    return {'files': len(files), 'bytes': sum(row['bytes'] for row in files.values()),
            'source_commit': COMMIT}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args()
    print(json.dumps(validate(args.root.resolve()), sort_keys=True))


if __name__ == '__main__':
    main()
