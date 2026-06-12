#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${WASM_TARGET:-wasm32-unknown-unknown}"

cd "$ROOT_DIR"

if [[ "$TARGET" != "wasm32-unknown-unknown" ]]; then
    echo "Unsupported WASM target for this gate: $TARGET" >&2
    echo "Set WASM_TARGET=wasm32-unknown-unknown or use a separate target-specific check." >&2
    exit 2
fi

if command -v rustup >/dev/null 2>&1; then
    rustup target add "$TARGET" >/dev/null
fi

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Dwarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Dwarnings"
fi

run_cargo() {
    env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" cargo "$@"
}

run_cargo check --target "$TARGET" \
    -p rmath \
    -p r-graphics-engine \
    -p r-device-android-headless

echo "WASM toolchain check passed for $TARGET."
