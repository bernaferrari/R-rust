#!/usr/bin/env bash
# R errors, return/break and restart handling need catch_unwind on Wasm too.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
WASM_TOOLCHAIN="nightly-2026-08-25"
export RUSTUP_TOOLCHAIN="$WASM_TOOLCHAIN"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target/wasm-unwind}"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Cpanic=unwind"
# build-std needs rust-src from this exact toolchain. Keep setup explicit in CI.
if ! rustup run "$WASM_TOOLCHAIN" rustc --version >/dev/null 2>&1; then
    echo "Install prerequisites: rustup toolchain install $WASM_TOOLCHAIN --profile minimal --component rust-src --target wasm32-unknown-unknown" >&2
    exit 2
fi
# Skip wasm-opt: the stable bundled optimiser can lag Rust's Wasm EH encoding.
# The release size gate measures the actual unoptimised-by-wasm-opt artifact.
# Optional GPU builds stay separate from the default CPU distribution.
if [[ -n "${RPORT_WASM_FEATURES:-}" ]]; then
    exec wasm-pack build crates/r-wasm --no-opt "$@" -- -Zbuild-std=std,panic_unwind --features "$RPORT_WASM_FEATURES"
else
    exec wasm-pack build crates/r-wasm --no-opt "$@" -- -Zbuild-std=std,panic_unwind
fi
