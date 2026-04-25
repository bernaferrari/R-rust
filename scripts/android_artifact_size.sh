#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${ANDROID_TARGET:-aarch64-linux-android}"
OUTPUT_DIR="$ROOT_DIR/target/performance"
CHECK=0
MAX_SO_BYTES="${RPORT_MAX_ANDROID_SO_BYTES:-52428800}"

usage() {
    cat <<'USAGE'
Usage: scripts/android_artifact_size.sh [OPTIONS]

Builds the Android UniFFI shared library in release mode and writes size
artifacts to target/performance.

Options:
  --output-dir DIR  Write report artifacts to DIR.
  --check           Fail if the shared library exceeds the configured threshold.
  -h, --help        Show this help.

Environment:
  ANDROID_TARGET                  Target triple, default aarch64-linux-android.
  RPORT_MAX_ANDROID_SO_BYTES      Size threshold, default 52428800 bytes.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --output-dir)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --check)
            CHECK=1
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

SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$SDK_ROOT" ]]; then
    for candidate in "$HOME/Library/Android/sdk" "$HOME/Android/Sdk" "/usr/local/lib/android/sdk"; do
        if [[ -d "$candidate" ]]; then
            SDK_ROOT="$candidate"
            break
        fi
    done
fi

if [[ -z "$SDK_ROOT" ]]; then
    echo "Set ANDROID_HOME or ANDROID_SDK_ROOT to point at a configured Android SDK." >&2
    exit 1
fi

NDK_BIN_DIR="$(find "$SDK_ROOT/ndk" -type d -path '*/toolchains/llvm/prebuilt/*/bin' 2>/dev/null | sort | head -n 1)"
if [[ -n "$NDK_BIN_DIR" ]]; then
    export PATH="$NDK_BIN_DIR:$PATH"
fi

if command -v rustup >/dev/null 2>&1; then
    rustup target add "$TARGET" >/dev/null
fi

target_env="$(printf '%s' "$TARGET" | tr '[:lower:]-' '[:upper:]_')"
linker_var="CARGO_TARGET_${target_env}_LINKER"
export "$linker_var=${TARGET}21-clang"

for tool in llvm-ar "${TARGET}21-clang"; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing Android toolchain helper on PATH: $tool" >&2
        exit 1
    fi
done

env RUSTFLAGS="${RUSTFLAGS:-}" cargo build --release --target "$TARGET" -p r-uniffi

artifact="$ROOT_DIR/target/$TARGET/release/libr_uniffi.so"
if [[ ! -f "$artifact" ]]; then
    echo "Missing Android artifact: $artifact" >&2
    exit 1
fi

size_bytes="$(wc -c < "$artifact" | tr -d '[:space:]')"
cat > "$OUTPUT_DIR/android-artifact-size.json" <<JSON
{
  "target": "$TARGET",
  "artifact": "$artifact",
  "size_bytes": $size_bytes,
  "threshold_bytes": $MAX_SO_BYTES
}
JSON

{
    echo "# Android Artifact Size"
    echo
    echo "| Target | Artifact | Size bytes | Threshold bytes |"
    echo "| --- | --- | ---: | ---: |"
    echo "| \`$TARGET\` | \`$artifact\` | $size_bytes | $MAX_SO_BYTES |"
} > "$OUTPUT_DIR/android-artifact-size.md"

echo "Android artifact size: $size_bytes bytes ($artifact)"

if [[ "$CHECK" -eq 1 && "$size_bytes" -gt "$MAX_SO_BYTES" ]]; then
    echo "Android artifact exceeds threshold: $size_bytes > $MAX_SO_BYTES" >&2
    exit 1
fi
