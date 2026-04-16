#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
OUT_DIR="$ROOT_DIR/bindings"
CHECK_ONLY=0
LANGUAGE="kotlin"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --check)
            CHECK_ONLY=1
            OUT_DIR="$(mktemp -d)"
            shift
            ;;
        --out-dir)
            OUT_DIR="$2"
            shift 2
            ;;
        --language)
            LANGUAGE="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

cleanup() {
    if [[ "$CHECK_ONLY" -eq 1 && -d "$OUT_DIR" ]]; then
        rm -rf "$OUT_DIR"
    fi
}
trap cleanup EXIT

echo "Generating UniFFI bindings..."

if ! command -v uniffi-bindgen >/dev/null 2>&1; then
    echo "Installing uniffi CLI (uniffi-bindgen)..."
    cargo install --locked --version 0.30.0 uniffi --features cli
fi

cd "$ROOT_DIR"
cargo build -p r-uniffi --lib

LIB_PATH="$(find "$TARGET_DIR" -type f \( -name 'libr_uniffi*.so' -o -name 'libr_uniffi*.dylib' -o -name 'libr_uniffi*.dll' -o -name 'r_uniffi*.dll' \) | sort | head -n 1)"
if [[ -z "$LIB_PATH" ]]; then
    echo "Error: Could not find the built r-uniffi library" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"

CRATE_CANDIDATES=(rport r_uniffi r-uniffi)
for crate_name in "${CRATE_CANDIDATES[@]}"; do
    if uniffi-bindgen generate --library "$LIB_PATH" --crate "$crate_name" --language "$LANGUAGE" --out-dir "$OUT_DIR/$LANGUAGE"; then
        echo "Generated $LANGUAGE bindings for crate $crate_name into $OUT_DIR/$LANGUAGE"
        echo "Done."
        exit 0
    fi
done

echo "Error: failed to generate UniFFI bindings from $LIB_PATH" >&2
exit 1
