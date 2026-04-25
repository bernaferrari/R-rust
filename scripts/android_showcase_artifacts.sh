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
import sys

out_dir = pathlib.Path(sys.argv[1])
transcript = out_dir / "showcase-transcript.txt"
line_plot = out_dir / "line-plot.png"
point_plot = out_dir / "point-plot.png"

for artifact in (transcript, line_plot, point_plot):
    if not artifact.is_file() or artifact.stat().st_size == 0:
        raise SystemExit(f"missing generated artifact: {artifact}")

for png in (line_plot, point_plot):
    if png.read_bytes()[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"not a PNG artifact: {png}")

text = transcript.read_text()
required = [
    "Pure-R package and S3 dispatch",
    "S3 dispatch:",
    "Typed result",
    "Parallel session isolation",
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
