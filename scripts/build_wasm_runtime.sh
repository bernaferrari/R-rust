#!/usr/bin/env bash
# R errors, return/break and restart handling need catch_unwind on Wasm too.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
WASM_TOOLCHAIN="nightly-2026-08-25"
export RUSTUP_TOOLCHAIN="$WASM_TOOLCHAIN"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target/wasm-unwind}"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Cpanic=unwind -Clink-arg=--max-memory=268435456"
# build-std needs rust-src from this exact toolchain. Keep setup explicit in CI.
if ! rustup run "$WASM_TOOLCHAIN" rustc --version >/dev/null 2>&1; then
    echo "Install prerequisites: rustup toolchain install $WASM_TOOLCHAIN --profile minimal --component rust-src --target wasm32-unknown-unknown" >&2
    exit 2
fi
# Skip wasm-pack's old bundled optimizer, which crashes on Rust's Wasm EH.
# Release assets use the separately pinned, verified Binaryen below.
# Optional GPU builds stay separate from the default CPU distribution.
if [[ -n "${RPORT_WASM_FEATURES:-}" ]]; then
    wasm-pack build crates/r-wasm --no-opt "$@" -- -Zbuild-std=std,panic_unwind --features "$RPORT_WASM_FEATURES"
else
    wasm-pack build crates/r-wasm --no-opt "$@" -- -Zbuild-std=std,panic_unwind
fi

OUTPUT_DIR="pkg"
OPTIMIZE=1
while (($# > 0)); do
    case "$1" in
        --out-dir) OUTPUT_DIR="$2"; shift 2 ;;
        --out-dir=*) OUTPUT_DIR="${1#--out-dir=}"; shift ;;
        --dev|--profiling) OPTIMIZE=0; shift ;;
        *) shift ;;
    esac
done
if [[ "$OPTIMIZE" == 1 ]]; then
    if [[ "$OUTPUT_DIR" != /* ]]; then OUTPUT_DIR="$ROOT_DIR/crates/r-wasm/$OUTPUT_DIR"; fi
    python3 "$ROOT_DIR/scripts/optimize_wasm_runtime.py" --package "$OUTPUT_DIR"
fi
