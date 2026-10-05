#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/scripts/conformance_artifacts.sh"
REPORT_DIR=""
STRICT=0
SUITE=all
SHARD_INDEX=0
SHARD_COUNT=1
# The original CI completed a numerical driver in13m30s. This newly declared
# bound preserves that opportunity; the legacy runner's120s is unchanged.
CASE_TIMEOUT="${RPORT_UPSTREAM_CASE_TIMEOUT:-1800}"

usage() {
    cat <<'USAGE'
Usage: scripts/upstream_core_slices.sh [--strict] [--report NEW_OR_EMPTY_DIR]
       [--suite all|curated|whole] [--shard-index N] [--shard-count N]
       [--timeout POSITIVE_SECONDS]

Runs unchanged pinned upstream drivers with the original strict merged-output
comparison. Each process has an owned deadline and durable START/FINISH logs.
Declared skips, completed behavior failures and incomplete execution remain
separate. Reports and exact engine artifacts are preserved on failure.
USAGE
}
while (($# > 0)); do
    case "$1" in
        --report|--suite|--shard-index|--shard-count|--timeout)
            (($# >= 2)) || { usage >&2; exit 2; }
            case "$1" in
                --report) REPORT_DIR="$2" ;;
                --suite) SUITE="$2" ;;
                --shard-index) SHARD_INDEX="$2" ;;
                --shard-count) SHARD_COUNT="$2" ;;
                --timeout) CASE_TIMEOUT="$2" ;;
            esac
            shift 2 ;;
        --strict) STRICT=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

[[ -n "$REPORT_DIR" ]] || REPORT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rport-upstream-evidence.XXXXXX")"
REPORT_DIR="$(python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).absolute())' "$REPORT_DIR")"
RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi
POLICY_ARGS=()
if [[ "$STRICT" == 1 ]]; then POLICY_ARGS+=(--strict); fi
if [[ "${RPORT_REQUIRE_PINNED_ORACLE:-0}" == 1 ]]; then POLICY_ARGS+=(--pinned); fi

# Capture all compile/corpus/policy inputs BEFORE building. Admission is checked
# again after compiling and after running; a moving source cannot borrow proof.
python3 "$ROOT_DIR/scripts/upstream_execution.py" prepare --root "$ROOT_DIR" \
    --report "$REPORT_DIR" --suite "$SUITE" --shard-index "$SHARD_INDEX" \
    --shard-count "$SHARD_COUNT" --timeout "$CASE_TIMEOUT" \
    --profile "$(conformance_profile)" --rustflags="$RUSTFLAGS_FOR_BUILD" \
    ${POLICY_ARGS[@]+"${POLICY_ARGS[@]}"}
printf 'INFO: durable upstream evidence: %s\n' "$REPORT_DIR"
python3 "$ROOT_DIR/scripts/validate_upstream_r_tests.py" --markdown "$REPORT_DIR/upstream-inventory.md"

if ! command -v Rscript >/dev/null 2>&1; then
    echo "ERROR: Rscript not found; evidence remains execution-incomplete." >&2
    if [[ "$STRICT" == 1 ]]; then exit 1; else exit 0; fi
fi
GNU_BIN="$(command -v Rscript)"
if [[ "${RPORT_REQUIRE_PINNED_ORACLE:-0}" == 1 ]]; then
    python3 "$ROOT_DIR/scripts/upstream_execution.py" process \
        --directory "$REPORT_DIR/oracle-validation" --cwd "$ROOT_DIR" --timeout "$CASE_TIMEOUT" \
        -- python3 "$ROOT_DIR/scripts/validate_r_oracle.py" --runtime "$GNU_BIN"
fi

# Keep actual Cargo compiler-artifact selection; never choose a library by
# timestamp or a stale guessed output path. Build logs also survive cancellation.
python3 "$ROOT_DIR/scripts/upstream_execution.py" process \
    --directory "$REPORT_DIR/library-build" --cwd "$ROOT_DIR" --timeout "$CASE_TIMEOUT" \
    --separate-streams -- env ROOT_DIR="$ROOT_DIR" RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" \
    bash -c 'source "$1"; conformance_build_rmath' _ "$ROOT_DIR/scripts/conformance_artifacts.sh"
RUST_RLIB="$(cat "$REPORT_DIR/library-build/stdout.log")"
[[ -f "$RUST_RLIB" ]] || { echo "ERROR: selected Cargo library is missing" >&2; exit 1; }
python3 "$ROOT_DIR/scripts/upstream_execution.py" seal-library \
    --root "$ROOT_DIR" --report "$REPORT_DIR" --rlib "$RUST_RLIB"
RUST_BIN="$REPORT_DIR/rust_runner"
python3 "$ROOT_DIR/scripts/upstream_execution.py" process \
    --directory "$REPORT_DIR/runner-build" --cwd "$ROOT_DIR" --timeout "$CASE_TIMEOUT" \
    -- rustc --edition=2024 "$ROOT_DIR/tests/conformance/src/main.rs" \
    -L "dependency=$(conformance_dependency_dir)" --extern "rmath=$RUST_RLIB" -o "$RUST_BIN"
python3 "$ROOT_DIR/scripts/upstream_execution.py" run --root "$ROOT_DIR" \
    --report "$REPORT_DIR" --gnu "$GNU_BIN" --rust "$RUST_BIN" --rlib "$RUST_RLIB"
