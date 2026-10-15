#!/usr/bin/env python3
"""Pack all original licensed font bytes using a bounded shared dictionary."""

import hashlib
import json
import lzma
from pathlib import Path


def main():
    directory = Path(__file__).resolve().parent
    names = ["DejaVuSans.ttf", "DejaVuSans-Bold.ttf", "DejaVuSans-Oblique.ttf", "DejaVuSans-BoldOblique.ttf"]
    originals = [(directory / name).read_bytes() for name in names]
    payload = b"".join(originals)
    compressed = lzma.compress(payload, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64,
        filters=[{"id": lzma.FILTER_LZMA2, "preset": 9, "dict_size": 1024 * 1024}])
    assert lzma.decompress(compressed) == payload
    (directory / "DejaVuSans.faces.xz").write_bytes(compressed)
    metadata = {
        "purpose": "Lossless storage only; every original TTF byte is preserved.",
        "regenerate": "python3 crates/r-graphics-engine/assets/compress_fonts.py",
        "compression": "XZ LZMA2 preset 9, fixed 1 MiB dictionary, CRC64",
        "license": "DejaVu-LICENSE.txt",
        "archive_bytes": len(compressed),
        "archive_sha256": hashlib.sha256(compressed).hexdigest(),
        "files": {name: {"original_bytes": len(data), "original_sha256": hashlib.sha256(data).hexdigest()}
            for name, data in zip(names, originals)},
    }
    (directory / "compressed-fonts.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"BUNDLED_FONT_BYTES_PASS faces=4 archive_bytes={len(compressed)} dictionary_bytes=1048576")


if __name__ == "__main__":
    main()
