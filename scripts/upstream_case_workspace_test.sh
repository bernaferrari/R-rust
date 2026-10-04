#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/upstream_case_workspace.sh"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/rport-upstream-workspace.XXXXXX")"
trap 'rm -rf "$TEST_ROOT"' EXIT

# Use spaces in paths and nested auxiliary fixtures. The child writes through
# both cwd and SRCDIR, like the real upstream drivers that generated vendor/NA.
SOURCE_DIR="$TEST_ROOT/immutable source"
mkdir -p "$SOURCE_DIR/nested data"
printf 'original fixture\n' >"$SOURCE_DIR/nested data/input.dat"
cat >"$SOURCE_DIR/case.R" <<'CASE'
#!/usr/bin/env bash
set -euo pipefail
[[ "$PWD" == "$SRCDIR" ]]
[[ "$TZ" == UTC && "$LC_ALL" == C && "$LANG" == C ]]
[[ ! -e NA && ! -e "$SRCDIR/generated" ]]
[[ "$(cat 'nested data/input.dat')" == 'original fixture' ]]
printf 'generated output\n' >NA
printf 'generated via SRCDIR\n' >"$SRCDIR/generated"
printf 'changed fixture\n' >"$SRCDIR/nested data/input.dat"
printf 'case finished\n'
CASE

for engine in stock rust; do
    upstream_stage_case_workspace "$SOURCE_DIR" "$TEST_ROOT/$engine"
done
upstream_run_case_workspace "$TEST_ROOT/stock" case.R bash >"$TEST_ROOT/stock.out"
# Rust must still see original fixture content and no stock-engine products.
upstream_run_case_workspace "$TEST_ROOT/rust" case.R bash >"$TEST_ROOT/rust.out"
cmp -s "$TEST_ROOT/stock.out" "$TEST_ROOT/rust.out"
[[ ! -e "$SOURCE_DIR/NA" && ! -e "$SOURCE_DIR/generated" ]]
[[ "$(cat "$SOURCE_DIR/nested data/input.dat")" == 'original fixture' ]]
cmp -s "$SOURCE_DIR/case.R" "$TEST_ROOT/stock/case.R"
cmp -s "$SOURCE_DIR/case.R" "$TEST_ROOT/rust/case.R"
printf 'independent upstream fixture workspace checks passed\n'
