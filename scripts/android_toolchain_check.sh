#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${ANDROID_TARGET:-aarch64-linux-android}"

cd "$ROOT_DIR"

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

cargo check -p r-device-android-headless
cargo check -p r-uniffi
cargo check --target "$TARGET" -p r-device-android-headless

echo "Android toolchain check passed for $TARGET."
