#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MODE="--check"
BIN_ARGS=()
while (($# > 0)); do
    case "$1" in
        --check|check)
            MODE="--check"
            shift
            ;;
        --allow-skip)
            BIN_ARGS+=(--allow-skip)
            shift
            ;;
        *)
            echo "usage: $0 [--check] [--allow-skip]" >&2
            echo "  --check       run the differential suite (default)" >&2
            echo "  --allow-skip  tolerate cases skipped because stock R failed" >&2
            echo "                (without it the harness exits non-zero on any skip)" >&2
            exit 2
            ;;
    esac
done

if ! command -v Rscript >/dev/null 2>&1; then
    echo "script-diff: SKIP (Rscript not installed)" >&2
    exit 0
fi

echo "script-diff parity: building harness"
env RUSTFLAGS="-Awarnings" cargo build -p rport-script-diff

echo "script-diff parity: running whole-script differential suite"
cargo run -p rport-script-diff -- ${BIN_ARGS[@]+"${BIN_ARGS[@]}"}

if [[ "$MODE" == "--check" ]]; then
    echo "script-diff parity: OK"
fi