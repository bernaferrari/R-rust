#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_ROOT="$ROOT_DIR/target/release-artifacts"
CHECK=0

usage() {
    cat <<'USAGE'
Usage: scripts/package_release_artifacts.sh [OPTIONS]

Builds a local release artifact bundle with docs, Kotlin UniFFI bindings,
Android shared library, size reports, and checksums.

Options:
  --output-dir DIR  Write release bundles under DIR.
  --check           Verify the expected bundle files are present and non-empty.
  -h, --help        Show this help.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --output-dir)
            OUTPUT_ROOT="$2"
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

version="$(awk '
    /^\[workspace\.package\]/ { in_workspace = 1; next }
    /^\[/ && in_workspace { in_workspace = 0 }
    in_workspace && $1 == "version" {
        gsub(/"/, "", $3)
        print $3
        exit
    }
' Cargo.toml)"

if [[ -z "$version" ]]; then
    echo "Could not read workspace.package.version from Cargo.toml" >&2
    exit 1
fi

BUNDLE_DIR="$OUTPUT_ROOT/rport-$version"
HOST_TARGET_DIR="$OUTPUT_ROOT/.host-bindgen-target-$version"
rm -rf "$BUNDLE_DIR"
rm -rf "$HOST_TARGET_DIR"
mkdir -p \
    "$BUNDLE_DIR/docs" \
    "$BUNDLE_DIR/oracle" \
    "$BUNDLE_DIR/bindings" \
    "$BUNDLE_DIR/android/jniLibs/arm64-v8a" \
    "$BUNDLE_DIR/reports"

CARGO_TARGET_DIR="$HOST_TARGET_DIR" scripts/generate_uniffi_bindings.sh --out-dir "$BUNDLE_DIR/bindings"
rm -rf "$HOST_TARGET_DIR"
scripts/android_artifact_size.sh --output-dir "$BUNDLE_DIR/reports" --check

android_so="$ROOT_DIR/target/${ANDROID_TARGET:-aarch64-linux-android}/release/libr_uniffi.so"
if [[ ! -f "$android_so" ]]; then
    echo "Missing Android shared library after build: $android_so" >&2
    exit 1
fi
cp -f "$android_so" "$BUNDLE_DIR/android/jniLibs/arm64-v8a/libr_uniffi.so"

cp -f README.md CHANGELOG.md NOTICE.md "$BUNDLE_DIR/"
cp -f oracle/r-oracle.json "$BUNDLE_DIR/oracle/"
cp -f \
    docs/android-embedding-api.md \
    docs/conformance.md \
    docs/performance.md \
    docs/release-gate.md \
    docs/release-packaging.md \
    docs/safe-api-audit.md \
    docs/upstream-port-map.md \
    docs/upstream-port-map.tsv \
    "$BUNDLE_DIR/docs/"

{
    echo "rport $version"
    echo
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "Android target: ${ANDROID_TARGET:-aarch64-linux-android}"
    echo
    find "$BUNDLE_DIR" -type f | sed "s#^$BUNDLE_DIR/##" | sort
} > "$BUNDLE_DIR/manifest.txt"

(
    cd "$BUNDLE_DIR"
    find . -type f ! -name manifest.sha256 -print0 \
        | sort -z \
        | xargs -0 shasum -a 256
) > "$BUNDLE_DIR/manifest.sha256"

if [[ "$CHECK" -eq 1 ]]; then
    required=(
        "$BUNDLE_DIR/README.md"
        "$BUNDLE_DIR/CHANGELOG.md"
        "$BUNDLE_DIR/NOTICE.md"
        "$BUNDLE_DIR/oracle/r-oracle.json"
        "$BUNDLE_DIR/docs/release-packaging.md"
        "$BUNDLE_DIR/docs/upstream-port-map.tsv"
        "$BUNDLE_DIR/android/jniLibs/arm64-v8a/libr_uniffi.so"
        "$BUNDLE_DIR/reports/android-artifact-size.json"
        "$BUNDLE_DIR/manifest.txt"
        "$BUNDLE_DIR/manifest.sha256"
    )
    for path in "${required[@]}"; do
        if [[ ! -s "$path" ]]; then
            echo "Missing or empty release artifact: $path" >&2
            exit 1
        fi
    done

    if ! find "$BUNDLE_DIR/bindings/kotlin" -type f -name '*.kt' -print -quit >/dev/null; then
        echo "No generated Kotlin bindings found under $BUNDLE_DIR/bindings/kotlin" >&2
        exit 1
    fi
fi

echo "Release artifact bundle: $BUNDLE_DIR"
echo "Manifest: $BUNDLE_DIR/manifest.txt"
echo "Checksums: $BUNDLE_DIR/manifest.sha256"
