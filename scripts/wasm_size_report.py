#!/usr/bin/env python3
"""Measure the sections and largest exact embedded files in a Wasm module."""

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SECTIONS = {
    0: 'custom', 1: 'type', 2: 'import', 3: 'function', 4: 'table',
    5: 'memory', 6: 'global', 7: 'export', 8: 'start', 9: 'element',
    10: 'code', 11: 'data', 12: 'data-count', 13: 'tag',
}


def section_length(module, offset):
    value = 0
    for shift in range(0, 35, 7):
        if offset == len(module):
            raise ValueError('truncated Wasm section length')
        byte = module[offset]
        offset += 1
        value |= (byte & 127) << shift
        if byte < 128 and value < (1 << 32):
            return value, offset
    raise ValueError('invalid Wasm section length')


def measure(module):
    if module[:8] != b'\0asm\x01\0\0\0':
        raise ValueError('expected a WebAssembly core module')
    sizes = Counter()
    data = b''
    offset = 8
    while offset < len(module):
        kind = module[offset]
        length, start = section_length(module, offset + 1)
        end = start + length
        if end > len(module):
            raise ValueError('truncated Wasm section')
        sizes[SECTIONS.get(kind, str(kind))] += length
        if kind == 11:
            data = module[start:end]
        offset = end
    return sizes, data


def embedded_files(data, root):
    paths = subprocess.check_output(['git', 'ls-files', '-z', '--', 'crates'], cwd=root)
    matches = []
    for name in paths.decode().split('\0'):
        path = root / name
        if not name or not path.is_file() or path.stat().st_size < 4096:
            continue
        contents = path.read_bytes()
        offset = data.find(contents)
        if offset >= 0:
            matches.append((len(contents), offset, name))

    # The same bytes can appear under multiple paths. Keep only disjoint
    # matches so aliases and files contained in a larger asset count once.
    ranges = []
    results = []
    for size, offset, name in sorted(matches, key=lambda row: (-row[0], row[2])):
        if any(offset < end and start < offset + size for start, end in ranges):
            continue
        ranges.append((offset, offset + size))
        results.append({'path': name, 'bytes': size})
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('module', type=Path)
    parser.add_argument('--source-root', type=Path, default=ROOT)
    parser.add_argument('--byte-limit', type=int, default=25 * 1024 * 1024)
    args = parser.parse_args()
    module = args.module.read_bytes()
    try:
        sizes, data = measure(module)
    except ValueError as error:
        parser.error(str(error))
    assets = embedded_files(data, args.source_root)
    print(json.dumps({
        'sha256': hashlib.sha256(module).hexdigest(),
        'bytes': len(module),
        'byte_limit': args.byte_limit,
        'headroom_bytes': args.byte_limit - len(module),
        'section_payload_bytes': dict(sizes),
        'module_and_section_headers_bytes': len(module) - sum(sizes.values()),
        'matched_asset_bytes': sum(asset['bytes'] for asset in assets),
        'largest_exact_assets': assets[:20],
        'asset_scope': 'Tracked files above 4 KiB with exact, contiguous, non-overlapping matches in the data section',
    }, indent=2))


if __name__ == '__main__':
    main()
