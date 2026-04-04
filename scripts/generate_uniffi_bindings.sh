#!/usr/bin/env bash
# Generate foreign language bindings using uniffi-bindgen
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

echo "Generating UniFFI bindings..."

# Install uniffi-bindgen if not present
if ! command -v uniffi-bindgen &> /dev/null; then
    echo "Installing uniffi-bindgen..."
    cargo install --version 0.30.0 uniffi_bindgen
fi

# Build the r-uniffi crate
cd "$ROOT_DIR"
cargo build -p r-uniffi --lib

# Find the built library
LIB_DIR=$(find "$ROOT_DIR/target" -name "libr_uniffi*" -type f | head -1 | xargs dirname)

if [ -z "$LIB_DIR" ]; then
    echo "Error: Could not find built library"
    exit 1
fi

# Generate Kotlin bindings
BINDINGS_DIR="$ROOT_DIR/bindings"
mkdir -p "$BINDINGS_DIR/kotlin"

uniffi-bindgen generate "$LIB_DIR/libr_uniffi.so" --language kotlin --out-dir "$BINDINGS_DIR/kotlin"
echo "Generated Kotlin bindings: $BINDINGS_DIR/kotlin/"

# Generate Swift bindings
mkdir -p "$BINDINGS_DIR/swift"
uniffi-bindgen generate "$LIB_DIR/libr_uniffi.dylib" --language swift --out-dir "$BINDINGS_DIR/swift" 2>/dev/null || echo "Swift bindings not available on this platform"

# Generate Python bindings
mkdir -p "$BINDINGS_DIR/python"
uniffi-bindgen generate "$LIB_DIR/libr_uniffi.so" --language python --out-dir "$BINDINGS_DIR/python"
echo "Generated Python bindings: $BINDINGS_DIR/python/"

echo "Done."
