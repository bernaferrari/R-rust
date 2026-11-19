#!/usr/bin/env python3
"""Pack the pinned GNU R TZif database for Wasm's filesystem-free runtime."""

import argparse
import hashlib
import io
from pathlib import Path
import struct
import zipfile
import zlib


ARCHIVE_SHA256 = 'b3822ddb638b2275bdbf8d3eb2dd61694369beeb869187c38548839f627b794e'
OUTPUT = Path(__file__).resolve().parents[1] / 'crates/rmath/src/tzone/zoneinfo'
MAX_ZONE_BYTES = 8192
MAX_BLOCK_BYTES = 32768


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path, help='src/extra/tzone/zoneinfo.zip from the pinned oracle')
    args = parser.parse_args()
    archive = args.archive.read_bytes()
    if hashlib.sha256(archive).hexdigest() != ARCHIVE_SHA256:
        parser.error('zoneinfo archive does not match the pinned GNU R source')

    blocks = [bytearray()]
    zones = {}
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
            if zone not in zones:
                if len(blocks[-1]) + len(zone) > MAX_BLOCK_BYTES:
                    blocks.append(bytearray())
                zones[zone] = (len(zones), len(blocks) - 1, len(blocks[-1]), len(zone))
                blocks[-1].extend(zone)
            index.append(f'{relative}\t{zones[zone][0]}\n')

    payload = bytearray()
    compressed_blocks = []
    for block in blocks:
        start = len(payload)
        payload.extend(zlib.compress(block, level=9))
        compressed_blocks.append((start, len(payload)))
    # The name index points into fixed-size records. Each record gives the
    # compressed block range and the zone's uncompressed offset and length.
    records = b''.join(
        struct.pack('<IIHH', *compressed_blocks[block], offset, length)
        for _, block, offset, length in zones.values()
    )

    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / 'index.txt').write_bytes(''.join(index).encode('ascii'))
    (OUTPUT / 'records.bin').write_bytes(records)
    (OUTPUT / 'zones.bin').write_bytes(payload)
    print(f'{len(index)} names, {len(zones)} distinct zones, '
          f'{len(blocks)} blocks, {len(payload)} compressed bytes')


if __name__ == '__main__':
    main()
