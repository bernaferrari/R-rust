#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GRADLEW="$ROOT_DIR/rstudio-mobile/gradlew"
REQUIRE_DEVICE=0
created_local_properties=0

detect_android_sdk() {
    local sdk_root="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
    if [[ -z "$sdk_root" ]]; then
        for candidate in "$HOME/Library/Android/sdk" "$HOME/Android/Sdk" "/usr/local/lib/android/sdk"; do
            if [[ -d "$candidate" ]]; then
                sdk_root="$candidate"
                break
            fi
        done
    fi
    if [[ -z "$sdk_root" ]]; then
        return 1
    fi
    export ANDROID_HOME="$sdk_root"
    export ANDROID_SDK_ROOT="$sdk_root"
    return 0
}

cleanup() {
    if [[ "$created_local_properties" -eq 1 ]]; then
        rm -f "$ROOT_DIR/rstudio-mobile/local.properties"
    fi
}
trap cleanup EXIT

select_java17() {
    if [[ -n "${JAVA_HOME_17_X64:-}" && -x "${JAVA_HOME_17_X64}/bin/java" ]]; then
        export JAVA_HOME="$JAVA_HOME_17_X64"
        export PATH="$JAVA_HOME/bin:$PATH"
        return
    fi
    if command -v /usr/libexec/java_home >/dev/null 2>&1; then
        local jh
        jh="$(/usr/libexec/java_home -v 17 2>/dev/null || true)"
        if [[ -n "$jh" && -x "$jh/bin/java" ]]; then
            export JAVA_HOME="$jh"
            export PATH="$JAVA_HOME/bin:$PATH"
            return
        fi
    fi
    for jh in /usr/lib/jvm/java-17-openjdk-amd64 /usr/lib/jvm/temurin-17-jdk-amd64; do
        if [[ -x "$jh/bin/java" ]]; then
            export JAVA_HOME="$jh"
            export PATH="$JAVA_HOME/bin:$PATH"
            return
        fi
    done
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --device)
            REQUIRE_DEVICE=1
            shift
            ;;
        --check)
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ ! -x "$GRADLEW" ]]; then
    echo "Expected Gradle wrapper at $GRADLEW" >&2
    exit 1
fi

select_java17
if ! detect_android_sdk; then
    echo "Set ANDROID_HOME or ANDROID_SDK_ROOT to point at a configured Android SDK." >&2
    exit 1
fi

if command -v java >/dev/null 2>&1; then
    java_major="$(java -version 2>&1 | awk -F[\".] '/version/ { if ($2 == 1) print $3; else print $2; exit }')"
    if [[ -n "$java_major" && "$java_major" -gt 21 ]]; then
        echo "Detected Java $java_major. Configure JAVA_HOME/JAVA_HOME_17_X64 to a Java 17 runtime for Gradle/Kotlin compatibility." >&2
        exit 1
    fi
fi

cd "$ROOT_DIR/rstudio-mobile"
if [[ ! -f local.properties ]]; then
    printf 'sdk.dir=%s\n' "$ANDROID_HOME" > local.properties
    created_local_properties=1
fi
"$GRADLEW" --no-daemon :app:assembleDebug

APK="$ROOT_DIR/rstudio-mobile/app/build/outputs/apk/debug/app-debug.apk"
if [[ ! -f "$APK" ]]; then
    echo "APK not found after assemble: $APK" >&2
    exit 1
fi

echo "Built debug APK: $APK"

if [[ "$REQUIRE_DEVICE" -eq 1 ]]; then
    if ! command -v adb >/dev/null 2>&1; then
        echo "adb is required for --device smoke validation." >&2
        exit 1
    fi

    device="$(adb devices | awk 'NR > 1 && $2 == "device" { print $1; exit }')"
    if [[ -z "$device" ]]; then
        echo "No connected adb device or emulator found." >&2
        exit 1
    fi

    adb install -r -g "$APK"
    adb shell am start -W -n com.rstudio.mobile/.MainActivity
    echo "Android device smoke launched on $device."
else
    echo "Packaging smoke complete. Use --device for on-device launch validation."
fi
