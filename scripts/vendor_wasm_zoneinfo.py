#!/usr/bin/env python3
"""Pack the pinned GNU R TZif database for Wasm's filesystem-free runtime."""

import argparse
import hashlib
import io
from pathlib import Path
import zipfile
import zlib


ARCHIVE_SHA256 = 'b3822ddb638b2275bdbf8d3eb2dd61694369beeb869187c38548839f627b794e'
OUTPUT = Path(__file__).resolve().parents[1] / 'crates/rmath/src/tzone/zoneinfo'
MAX_ZONE_BYTES = 8192


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path, help='src/extra/tzone/zoneinfo.zip from the pinned oracle')
    args = parser.parse_args()
    archive = args.archive.read_bytes()
    if hashlib.sha256(archive).hexdigest() != ARCHIVE_SHA256:
        parser.error('zoneinfo archive does not match the pinned GNU R source')

    payload = bytearray()
    offsets = {}
    index = []
    with zipfile.ZipFile(io.BytesIO(archive)) as source:
        for name in sorted(source.namelist()):
            relative = name.removeprefix('zoneinfo/')
            if name.endswith('/'):
                continue
            zone = source.read(name)
            if not zone.startswith(b'TZif'):
                continue
            if len(zone) > MAX_ZONE_BYTES:
                parser.error(f'{relative} exceeds the Wasm zone size limit')
            if zone not in offsets:
                compressed = zlib.compress(zone, level=9)
                offsets[zone] = (len(payload), len(compressed))
                payload.extend(compressed)
            offset, length = offsets[zone]
            index.append(f'{relative}\t{offset}\t{length}\n')

    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / 'index.txt').write_bytes(''.join(index).encode('ascii'))
    (OUTPUT / 'zones.bin').write_bytes(payload)
    print(f'{len(index)} names, {len(offsets)} distinct zones, {len(payload)} compressed bytes')


if __name__ == '__main__':
    main()
