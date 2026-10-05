#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/rport-conformance-artifacts.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

ROOT_DIR="$TEST_ROOT/repository"
mkdir -p "$ROOT_DIR/target/debug/deps"
unset CARGO_TARGET_DIR RPORT_CONFORMANCE_PROFILE
source "$SCRIPT_DIR/conformance_artifacts.sh"

[[ "$CARGO_TARGET_DIR" == "$ROOT_DIR/target/conformance" ]]

# A stale default-target artifact must not win when Cargo is configured to use
# a relative target directory.
printf 'stale\n' >"$ROOT_DIR/target/debug/deps/librmath-stale.rlib"
CARGO_TARGET_DIR="isolated-target"
mkdir -p "$ROOT_DIR/isolated-target/debug/deps"
printf 'fresh\n' >"$ROOT_DIR/isolated-target/debug/deps/librmath-fresh.rlib"

expected="$ROOT_DIR/isolated-target/debug/deps/librmath-fresh.rlib"
actual="$(conformance_find_rmath_rlib)"
[[ "$actual" == "$expected" ]]
[[ "$(conformance_dependency_dir)" == "$ROOT_DIR/isolated-target/debug/deps" ]]

# Absolute target directories must be preserved verbatim.
CARGO_TARGET_DIR="$TEST_ROOT/absolute-target"
mkdir -p "$CARGO_TARGET_DIR/debug/deps"
printf 'absolute\n' >"$CARGO_TARGET_DIR/debug/deps/librmath-absolute.rlib"
[[ "$(conformance_find_rmath_rlib)" == "$CARGO_TARGET_DIR/debug/deps/librmath-absolute.rlib" ]]

# A release selection must resolve only release artifacts. A newer debug
# library is not compatible evidence for the explicitly selected profile.
RPORT_CONFORMANCE_PROFILE=release
mkdir -p "$CARGO_TARGET_DIR/release/deps"
printf 'release\n' >"$CARGO_TARGET_DIR/release/deps/librmath-release.rlib"
[[ "$(conformance_find_rmath_rlib)" == "$CARGO_TARGET_DIR/release/deps/librmath-release.rlib" ]]
[[ "$(conformance_dependency_dir)" == "$CARGO_TARGET_DIR/release/deps" ]]

# Profile selection reaches Cargo as one argument, through the repository
# wrapper. Preserve caller arguments, spaces and RUSTFLAGS in both profiles.
mkdir -p "$ROOT_DIR/scripts"
cat >"$ROOT_DIR/scripts/cargo_dev.sh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$PWD" "${RUSTFLAGS:-}" "$@" >"$RPORT_TEST_CARGO_RECEIPT"
SH
chmod +x "$ROOT_DIR/scripts/cargo_dev.sh"
export RPORT_TEST_CARGO_RECEIPT="$TEST_ROOT/cargo.args"
RUSTFLAGS="-D warnings" conformance_cargo build -p rmath --features "fixture flag"
printf '%s\n' "$(cd "$ROOT_DIR" && pwd)" '-D warnings' build --release -p rmath --features "fixture flag" >"$TEST_ROOT/expected.args"
diff -u "$TEST_ROOT/expected.args" "$RPORT_TEST_CARGO_RECEIPT"
RPORT_CONFORMANCE_PROFILE=debug
RUSTFLAGS="-D warnings" conformance_cargo build -p rmath --features "fixture flag"
printf '%s\n' "$(cd "$ROOT_DIR" && pwd)" '-D warnings' build -p rmath --features "fixture flag" >"$TEST_ROOT/expected.args"
diff -u "$TEST_ROOT/expected.args" "$RPORT_TEST_CARGO_RECEIPT"

# Invalid selection fails; it must not silently use another build's library.
RPORT_CONFORMANCE_PROFILE=not-a-profile
if conformance_find_rmath_rlib >/dev/null 2>&1; then
    echo "invalid profile selected an artifact" >&2
    exit 1
fi

echo "conformance artifact path checks passed"
