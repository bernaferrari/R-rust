#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
OUT_DIR="$TARGET_DIR/bindings"
CHECK_ONLY=0
LANGUAGE="kotlin"
CHECKED_IN_DIR="$ROOT_DIR/apps/workbench/app/generated"
BUILD_RECEIPT=""

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
    rm -f "$BUILD_RECEIPT"
    if [[ "$CHECK_ONLY" -eq 1 && -d "$OUT_DIR" ]]; then
        rm -rf "$OUT_DIR"
    fi
}
trap cleanup EXIT
BUILD_RECEIPT="$(mktemp)"

echo "Generating UniFFI bindings..."

UNIFFI_VERSION="$({
    awk '
        $0 == "name = \"uniffi\"" { in_uniffi = 1; next }
        in_uniffi && /^version = / {
            gsub(/\"/, "", $3)
            print $3
            exit
        }
    ' "$ROOT_DIR/Cargo.lock"
})"
if [[ -z "$UNIFFI_VERSION" ]]; then
    echo "Error: Could not resolve the UniFFI version from Cargo.lock" >&2
    exit 1
fi

INSTALLED_UNIFFI_VERSION=""
if command -v uniffi-bindgen >/dev/null 2>&1; then
    INSTALLED_UNIFFI_VERSION="$(uniffi-bindgen --version 2>/dev/null | awk '{print $2}')"
fi
if [[ "$INSTALLED_UNIFFI_VERSION" != "$UNIFFI_VERSION" ]]; then
    echo "Installing uniffi-bindgen $UNIFFI_VERSION..."
    "$SCRIPT_DIR/cargo_dev.sh" install --locked --force --version "$UNIFFI_VERSION" uniffi --features cli
fi

cd "$ROOT_DIR"
RPORT_CARGO_ARTIFACTS_JSON="$BUILD_RECEIPT" \
    "$SCRIPT_DIR/cargo_dev.sh" build -p r-uniffi --lib --message-format=json > "$BUILD_RECEIPT"
LIB_PATH="$(python3 "$SCRIPT_DIR/conformance_cargo_artifact.py" "$BUILD_RECEIPT" r_uniffi --shared)"

mkdir -p "$OUT_DIR"

CRATE_CANDIDATES=(r_uniffi rport r-uniffi)
GENERATED=0
for crate_name in "${CRATE_CANDIDATES[@]}"; do
    if uniffi-bindgen generate --no-format --library "$LIB_PATH" --crate "$crate_name" --language "$LANGUAGE" --out-dir "$OUT_DIR/$LANGUAGE"; then
        echo "Generated $LANGUAGE bindings for crate $crate_name into $OUT_DIR/$LANGUAGE"
        GENERATED=1
        break
    fi
done

if [[ "$GENERATED" -ne 1 ]]; then
    echo "Error: failed to generate UniFFI bindings from $LIB_PATH" >&2
    exit 1
fi

# UniFFI's no-format templates contain trailing spaces and blank lines. Strip
# those unstable details so generation is byte-for-byte reproducible.
while IFS= read -r -d '' generated_file; do
    perl -0777 -pi -e 's/[ \t]+$//mg; s/\s+\z/\n/' "$generated_file"
done < <(find "$OUT_DIR/$LANGUAGE" -type f -print0)

if [[ "$CHECK_ONLY" -eq 1 ]]; then
    if ! diff -ru "$CHECKED_IN_DIR/$LANGUAGE" "$OUT_DIR/$LANGUAGE"; then
        echo "Error: checked-in $LANGUAGE UniFFI bindings are stale" >&2
        echo "Run scripts/generate_uniffi_bindings.sh --out-dir $CHECKED_IN_DIR" >&2
        exit 1
    fi
    echo "Checked-in $LANGUAGE bindings are current."
fi

echo "Done."
