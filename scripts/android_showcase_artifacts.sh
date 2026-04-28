#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/target/android-showcase"
CHECK_ONLY=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --check)
            CHECK_ONLY=1
            shift
            ;;
        --out-dir)
            OUT_DIR="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

cd "$ROOT_DIR"
cargo run -p r-embed --example android_showcase -- "$OUT_DIR"

python3 - "$OUT_DIR" <<'PY'
import pathlib
import struct
import sys
import zlib

out_dir = pathlib.Path(sys.argv[1])
transcript = out_dir / "showcase-transcript.txt"
line_plot = out_dir / "line-plot.png"
point_plot = out_dir / "point-plot.png"

for artifact in (transcript, line_plot, point_plot):
    if not artifact.is_file() or artifact.stat().st_size == 0:
        raise SystemExit(f"missing generated artifact: {artifact}")

def decode_png_rgba(path):
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"not a PNG artifact: {path}")

    offset = 8
    width = height = color_type = bit_depth = None
    compressed = bytearray()
    while offset < len(data):
        if offset + 8 > len(data):
            raise SystemExit(f"truncated PNG chunk header: {path}")
        length = struct.unpack(">I", data[offset : offset + 4])[0]
        chunk_type = data[offset + 4 : offset + 8]
        chunk = data[offset + 8 : offset + 8 + length]
        offset += 12 + length
        if chunk_type == b"IHDR":
            width, height, bit_depth, color_type, _, _, _ = struct.unpack(">IIBBBBB", chunk)
        elif chunk_type == b"IDAT":
            compressed.extend(chunk)
        elif chunk_type == b"IEND":
            break

    if (width, height, bit_depth, color_type) != (720, 480, 8, 6):
        raise SystemExit(
            f"unexpected PNG format for {path}: "
            f"{width}x{height}, depth={bit_depth}, color={color_type}"
        )

    raw = zlib.decompress(bytes(compressed))
    stride = width * 4
    rows = []
    prev = bytearray(stride)
    cursor = 0
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        row = bytearray(raw[cursor : cursor + stride])
        cursor += stride
        for i, value in enumerate(row):
            left = row[i - 4] if i >= 4 else 0
            up = prev[i]
            up_left = prev[i - 4] if i >= 4 else 0
            if filter_type == 0:
                pass
            elif filter_type == 1:
                row[i] = (value + left) & 0xFF
            elif filter_type == 2:
                row[i] = (value + up) & 0xFF
            elif filter_type == 3:
                row[i] = (value + ((left + up) // 2)) & 0xFF
            elif filter_type == 4:
                p = left + up - up_left
                pa = abs(p - left)
                pb = abs(p - up)
                pc = abs(p - up_left)
                predictor = left if pa <= pb and pa <= pc else up if pb <= pc else up_left
                row[i] = (value + predictor) & 0xFF
            else:
                raise SystemExit(f"unsupported PNG filter {filter_type} in {path}")
        rows.append(bytes(row))
        prev = row
    return width, height, b"".join(rows)


def count_pixels(rgba, predicate):
    return sum(1 for i in range(0, len(rgba), 4) if predicate(rgba[i : i + 4]))


def count_region(rgba, width, x0, y0, x1, y1, predicate):
    count = 0
    for y in range(y0, min(y1, len(rgba) // (width * 4))):
        for x in range(x0, min(x1, width)):
            idx = (y * width + x) * 4
            if predicate(rgba[idx : idx + 4]):
                count += 1
    return count


line_w, line_h, line_rgba = decode_png_rgba(line_plot)
point_w, point_h, point_rgba = decode_png_rgba(point_plot)

line_blue = count_pixels(line_rgba, lambda px: px[2] > 150 and px[0] < 140 and px[1] < 180 and px[3] > 0)
point_green = count_pixels(point_rgba, lambda px: px[1] > 100 and px[0] < 140 and px[2] < 140 and px[3] > 0)
line_title = count_region(line_rgba, line_w, 0, 0, line_w, 58, lambda px: px != b"\xff\xff\xff\xff")
point_body = count_region(point_rgba, point_w, 0, 0, point_w, point_h, lambda px: px != b"\xff\xff\xff\xff")

if line_blue <= 20:
    raise SystemExit(f"line plot did not render enough blue series pixels: {line_blue}")
if point_green <= 20:
    raise SystemExit(f"point plot did not render enough green series pixels: {point_green}")
if line_title <= 20:
    raise SystemExit(f"line plot title/header region appears blank: {line_title}")
if point_body <= 200:
    raise SystemExit(f"point plot appears mostly blank: {point_body}")

text = transcript.read_text()
required = [
    "Pure-R package and S3 dispatch",
    "value: string vector [Some(\"S3 dispatch: androiddemo\")]",
    "Typed result",
    "value: real scalar Some(42.0)",
    "Parallel session isolation",
    "Session A: tab_value=[1] \"A\", other_tab_value visible=[1] FALSE",
    "Session B: tab_value=[1] \"B\", other_tab_value visible=[1] FALSE",
    "Cancellation",
    "operation cancelled",
]
missing = [needle for needle in required if needle not in text]
if missing:
    raise SystemExit(f"showcase transcript missing: {missing}")

print(f"Android showcase artifacts OK: {out_dir}")
PY

if [[ "$CHECK_ONLY" -eq 0 ]]; then
    echo "Transcript: $OUT_DIR/showcase-transcript.txt"
    echo "Plots: $OUT_DIR/line-plot.png $OUT_DIR/point-plot.png"
fi
