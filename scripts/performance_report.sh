#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT_DIR/target/performance"
ITERATIONS=100
CHECK=0
RUN_ANDROID_SIZE=1
RUN_STOCK_R=1

usage() {
    cat <<'USAGE'
Usage: scripts/performance_report.sh [OPTIONS]

Runs representative embedding performance probes and writes Markdown/JSON
artifacts under target/performance.

Options:
  --iterations N        Iterations for steady-state probes, default 100.
  --quick               Use 10 iterations for a fast local regression check.
  --output-dir DIR      Write report artifacts to DIR.
  --check               Enforce loose thresholds for obvious regressions.
  --skip-android-size   Skip Android release shared-library size measurement.
  --skip-stock-r        Skip stock GNU R versus Rust comparison.
  -h, --help            Show this help.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --iterations)
            ITERATIONS="$2"
            shift 2
            ;;
        --quick)
            ITERATIONS=10
            shift
            ;;
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --check)
            CHECK=1
            shift
            ;;
        --skip-android-size)
            RUN_ANDROID_SIZE=0
            shift
            ;;
        --skip-stock-r)
            RUN_STOCK_R=0
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

cd "$ROOT_DIR"
mkdir -p "$OUTPUT_DIR"

probe_args=(--iterations "$ITERATIONS" --output-dir "$OUTPUT_DIR")
if [[ "$CHECK" -eq 1 ]]; then
    probe_args+=(--check)
fi

env RUSTFLAGS="${RUSTFLAGS:-}" cargo run -p r-embed --example performance_probe --release -- "${probe_args[@]}"

if [[ "$RUN_STOCK_R" -eq 1 ]]; then
    stock_args=(--iterations "$ITERATIONS" --output-dir "$OUTPUT_DIR/stock-r")
    if [[ "$CHECK" -eq 1 ]]; then
        stock_args+=(--check --strict)
    fi
    scripts/compare_stock_r_performance.sh "${stock_args[@]}"
fi

if [[ "$RUN_ANDROID_SIZE" -eq 1 ]]; then
    size_args=(--output-dir "$OUTPUT_DIR")
    if [[ "$CHECK" -eq 1 ]]; then
        size_args+=(--check)
    fi
    scripts/android_artifact_size.sh "${size_args[@]}"
fi

echo
echo "Performance report artifacts:"
echo "  $OUTPUT_DIR/performance-summary.md"
echo "  $OUTPUT_DIR/performance-summary.json"
if [[ "$RUN_ANDROID_SIZE" -eq 1 ]]; then
    echo "  $OUTPUT_DIR/android-artifact-size.md"
    echo "  $OUTPUT_DIR/android-artifact-size.json"
fi
if [[ "$RUN_STOCK_R" -eq 1 ]]; then
    echo "  $OUTPUT_DIR/stock-r/stock-r-performance-summary.md"
    echo "  $OUTPUT_DIR/stock-r/stock-r-performance-summary.json"
fi
