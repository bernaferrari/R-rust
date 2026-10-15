#!/usr/bin/env python3
"""Generate small compressed XDR v2 RDS fixtures for native admission tests.

Validate the three ordinary records with the pinned GNU R readRDS oracle.
The oversized XZ dictionary fixture is only for the bounded Rust decoder.
"""
from pathlib import Path
import bz2
import gzip
import lzma
import struct
import zlib

root = Path(__file__).resolve().parents[1]
output = root / "crates/r-embed/tests/fixtures/native-codec-admission"
output.mkdir(parents=True, exist_ok=True)
data = b"X\n" + struct.pack(">5i", 2, 0x40700, 0x20300, 24, 2 * 1024 * 1024)
data += bytes(2 * 1024 * 1024)
(output / "gzip.rds").write_bytes(gzip.compress(data, mtime=0))
(output / "bzip2.rds").write_bytes(bz2.compress(data))
xz = lzma.compress(data)
(output / "xz.rds").write_bytes(xz)
# One LZMA2 filter, eight-byte block-header body. Advertise its maximum
# dictionary before any data decode; repair the block header CRC accordingly.
assert xz[12:16] == bytes([2, 0, 0x21, 1])
large = bytearray(xz)
large[16] = 40
large[20:24] = struct.pack("<I", zlib.crc32(large[12:20]))
(output / "oversized-dictionary.xz").write_bytes(large)
