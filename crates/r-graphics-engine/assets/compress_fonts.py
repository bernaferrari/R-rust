#!/usr/bin/env python3
"""Regenerate lossless embedded fonts from the retained, licensed TTF originals."""

import gzip
import hashlib
import json
from pathlib import Path
import sys
import zlib


def main():
    directory = Path(__file__).resolve().parent
    metadata = {
        "purpose": "Lossless storage only; every original TTF byte is preserved.",
        "regenerate": "python3 crates/r-graphics-engine/assets/compress_fonts.py",
        "compression": "gzip level 9, mtime 0, OS 255; deflate output may vary by zlib version",
        "python": sys.version.split()[0],
        "zlib": zlib.ZLIB_VERSION,
        "license": "DejaVu-LICENSE.txt",
        "files": {},
    }
    for name in [
        "DejaVuSans.ttf",
        "DejaVuSans-Bold.ttf",
        "DejaVuSans-Oblique.ttf",
        "DejaVuSans-BoldOblique.ttf",
    ]:
        original = (directory / name).read_bytes()
        compressed = bytearray(gzip.compress(original, compresslevel=9, mtime=0))
        compressed[9] = 255
        compressed = bytes(compressed)
        assert gzip.decompress(compressed) == original
        (directory / (name + ".gz")).write_bytes(compressed)
        metadata["files"][name] = {
            "original_bytes": len(original),
            "original_sha256": hashlib.sha256(original).hexdigest(),
            "gzip_bytes": len(compressed),
            "gzip_sha256": hashlib.sha256(compressed).hexdigest(),
        }
    (directory / "compressed-fonts.json").write_text(
        json.dumps(metadata, indent=2) + "\n", encoding="utf-8"
    )
    print("BUNDLED_FONT_BYTES_PASS faces=4")


if __name__ == "__main__":
    main()
