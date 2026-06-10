#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MODE="--check"
while (($# > 0)); do
    case "$1" in
        --check|check)
            MODE="--check"
            shift
            ;;
        *)
            echo "usage: $0 [--check]" >&2
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
cargo run -p rport-script-diff

if [[ "$MODE" == "--check" ]]; then
    echo "script-diff parity: OK"
fi