#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/rport-conformance-artifacts.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

ROOT_DIR="$TEST_ROOT/repository"
mkdir -p "$ROOT_DIR/target/debug/deps"
unset CARGO_TARGET_DIR
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

echo "conformance artifact path checks passed"
