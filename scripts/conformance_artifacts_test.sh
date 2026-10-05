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

# Cargo may reuse an older exact variant while a different feature build has
# a newer sibling. Selection must follow the emitted cached artifact, not mtime.
CARGO_TARGET_DIR="$TEST_ROOT/exact artifact target"
mkdir -p "$CARGO_TARGET_DIR/debug/deps"
expected="$CARGO_TARGET_DIR/debug/deps/librmath-cached-correct.rlib"
wrong="$CARGO_TARGET_DIR/debug/deps/librmath-feature-wrong.rlib"
printf 'cached correct\n' >"$expected"
printf 'different variant\n' >"$wrong"
touch -t 202001010000 "$expected"
touch -t 202101010000 "$wrong"
export RPORT_TEST_CARGO_OUTPUT="$TEST_ROOT/cargo-output.json"
python3 - "$expected" >"$RPORT_TEST_CARGO_OUTPUT" <<'PYJSON'
import json,sys
print(json.dumps({"reason":"compiler-artifact", "package_id":"path+file:///fixture#rmath@0.1.0", "target":{"name":"rmath","kind":["lib"],"crate_types":["lib"]}, "filenames":[sys.argv[1]], "profile":{"test":False}, "fresh":True}))
print("0 obsolete binaries, 0 old warning logs; 0.00 GiB apparent size.")
PYJSON
cat >>"$ROOT_DIR/scripts/cargo_dev.sh" <<'SH'
cat "$RPORT_TEST_CARGO_OUTPUT"
exit "${RPORT_TEST_CARGO_STATUS:-0}"
SH
cp -f "$SCRIPT_DIR/conformance_cargo_artifact.py" "$ROOT_DIR/scripts/"
actual="$(RUSTFLAGS="-D warnings" conformance_build_rmath --features "fixture flag")"
if [[ "$actual" != "$expected" ]]; then
    printf 'Wrong Cargo artifact: expected %s; selected %s\n' "$expected" "$actual" >&2
    exit 1
fi

# The exact build helper preserves caller settings and arguments too.
printf '%s\n' "$(cd "$ROOT_DIR" && pwd)" '-D warnings' build -p rmath --features "fixture flag" --message-format=json >"$TEST_ROOT/expected.args"
diff -u "$TEST_ROOT/expected.args" "$RPORT_TEST_CARGO_RECEIPT"
RPORT_CONFORMANCE_PROFILE=release
actual="$(conformance_build_rmath --features "fixture flag")"
[[ "$actual" == "$expected" ]]
printf '%s\n' "$(cd "$ROOT_DIR" && pwd)" '' build --release -p rmath --features "fixture flag" --message-format=json >"$TEST_ROOT/expected.args"
diff -u "$TEST_ROOT/expected.args" "$RPORT_TEST_CARGO_RECEIPT"
RPORT_CONFORMANCE_PROFILE=debug

# Failed Cargo cannot certify an already emitted library.
export RPORT_TEST_CARGO_STATUS=17
status=0
conformance_build_rmath >"$TEST_ROOT/selected" 2>"$TEST_ROOT/error" || status=$?
[[ "$status" == 17 && ! -s "$TEST_ROOT/selected" ]]
unset RPORT_TEST_CARGO_STATUS

# Duplicate messages for the same artifact are harmless; distinct variants
# are ambiguous. No-artifact, test/executable and dependency targets cannot win.
cp -f "$RPORT_TEST_CARGO_OUTPUT" "$TEST_ROOT/valid-output"
cat "$TEST_ROOT/valid-output" >>"$RPORT_TEST_CARGO_OUTPUT"
[[ "$(conformance_build_rmath)" == "$expected" ]]
python3 - "$expected" "$wrong" "$RPORT_TEST_CARGO_OUTPUT" <<'PYJSON'
import json,sys
from pathlib import Path
valid = {"reason":"compiler-artifact", "target":{"name":"rmath","kind":["lib"]}, "profile":{"test":False}, "filenames":[sys.argv[1]], "fresh":True}
Path(sys.argv[3]).write_text(json.dumps(valid)+"\n"+json.dumps(dict(valid,filenames=[sys.argv[2]]))+"\n")
PYJSON
if conformance_build_rmath >"$TEST_ROOT/selected" 2>"$TEST_ROOT/error"; then
    echo "ambiguous variants admitted" >&2; exit 1
fi
[[ ! -s "$TEST_ROOT/selected" ]]
python3 - "$expected" "$RPORT_TEST_CARGO_OUTPUT" <<'PYJSON'
import json,sys
from pathlib import Path
base={"reason":"compiler-artifact", "target":{"name":"rmath","kind":["lib"]}, "profile":{"test":False}, "filenames":[sys.argv[1]]}
excluded=[dict(base,profile={"test":True}),dict(base,target={"name":"rmath","kind":["bin"]}),dict(base,executable="/fixture/test"),dict(base,target={"name":"rmath_nmath","kind":["lib"]})]
Path(sys.argv[2]).write_text("\n".join(map(json.dumps,excluded))+"\n")
PYJSON
if conformance_build_rmath >"$TEST_ROOT/selected" 2>"$TEST_ROOT/error"; then
    echo "non-library targets admitted" >&2; exit 1
fi
[[ ! -s "$TEST_ROOT/selected" ]]
printf 'pruning receipt only\n' >"$RPORT_TEST_CARGO_OUTPUT"
if conformance_build_rmath >"$TEST_ROOT/selected" 2>"$TEST_ROOT/error"; then
    echo "missing artifact admitted" >&2; exit 1
fi
[[ ! -s "$TEST_ROOT/selected" ]]
printf '{"reason":\n' >"$RPORT_TEST_CARGO_OUTPUT"
if conformance_build_rmath >"$TEST_ROOT/selected" 2>"$TEST_ROOT/error"; then
    echo "invalid JSON admitted" >&2; exit 1
fi
[[ ! -s "$TEST_ROOT/selected" ]]

# An unsuccessful JSON build-finished record also invalidates emitted output.
cp -f "$TEST_ROOT/valid-output" "$RPORT_TEST_CARGO_OUTPUT"
printf '{"reason":"build-finished","success":false}\n' >>"$RPORT_TEST_CARGO_OUTPUT"
if conformance_build_rmath >"$TEST_ROOT/selected" 2>"$TEST_ROOT/error"; then
    echo "unsuccessful build-finished admitted" >&2; exit 1
fi
[[ ! -s "$TEST_ROOT/selected" ]]
python3 - "$RPORT_TEST_CARGO_OUTPUT" "$TEST_ROOT/missing.rlib" <<'PYJSON'
import json,sys
from pathlib import Path
Path(sys.argv[1]).write_text(json.dumps({"reason":"compiler-artifact","target":{"name":"rmath","kind":["lib"]},"profile":{"test":False},"filenames":[sys.argv[2]]})+"\n")
PYJSON
if conformance_build_rmath >"$TEST_ROOT/selected" 2>"$TEST_ROOT/error"; then
    echo "nonexistent emitted file admitted" >&2; exit 1
fi
[[ ! -s "$TEST_ROOT/selected" ]]

# Invalid selection fails; it must not silently use another build's library.
RPORT_CONFORMANCE_PROFILE=not-a-profile
if conformance_find_rmath_rlib >/dev/null 2>&1; then
    echo "invalid profile selected an artifact" >&2
    exit 1
fi

echo "conformance artifact path checks passed"
