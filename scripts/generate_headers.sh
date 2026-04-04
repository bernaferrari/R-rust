#!/usr/bin/env bash
# Generate C headers from Rust FFI functions using cbindgen
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

echo "Generating C headers..."

# Install cbindgen if not present
if ! command -v cbindgen &> /dev/null; then
    echo "Installing cbindgen..."
    cargo install cbindgen
fi

# Generate header for rmath FFI functions
cd "$ROOT_DIR/rmath-rs"
cbindgen --config "$ROOT_DIR/cbindgen.toml" --output "$ROOT_DIR/rmath.h" --crate rmath
echo "Generated: $ROOT_DIR/rmath.h"

echo "Done."
