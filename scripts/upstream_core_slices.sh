#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASES_DIR="$ROOT_DIR/tests/upstream-core/cases"
XFAIL_FILE="$ROOT_DIR/tests/upstream-core/xfail.tsv"
RUST_RUNNER_SRC="$ROOT_DIR/tests/conformance/src/main.rs"
REPORT_DIR=""

usage() {
    cat <<'USAGE'
Usage: scripts/upstream_core_slices.sh [--report DIR]

Runs curated GNU R upstream evaluator/arithmetic slices against stock R and the
Rust runtime. These are adapted from r-source/tests/*.R to stay inside the
currently supported embedded runtime surface.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --report)
            if (($# < 2)); then
                usage >&2
                exit 2
            fi
            REPORT_DIR="$2"
            shift 2
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

if ! command -v Rscript >/dev/null 2>&1; then
    echo "SKIP: Rscript not found; upstream core slices require stock C R." >&2
    exit 0
fi

if [[ ! -d "$CASES_DIR" ]]; then
    echo "ERROR: missing upstream slice cases directory: $CASES_DIR" >&2
    exit 1
fi

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi

find_rust_rlib() {
    local found=""
    shopt -s nullglob
    local rust_rlibs=(
        "$ROOT_DIR"/target/debug/deps/librmath-*.rlib
        "$ROOT_DIR"/target/debug/deps/librmath.rlib
    )
    shopt -u nullglob
    if (( ${#rust_rlibs[@]} > 0 )); then
        found="$(ls -t "${rust_rlibs[@]}" 2>/dev/null | head -n1)"
    fi
    printf '%s' "$found"
}

echo "INFO: building Rust rmath artifact for upstream slice runner." >&2
(cd "$ROOT_DIR" && env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" cargo build -p rmath >/dev/null)

RUST_RLIB="$(find_rust_rlib)"
if [[ -z "$RUST_RLIB" ]]; then
    echo "ERROR: Rust rmath artifact missing after build." >&2
    exit 1
fi

RUNNER_TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rport-upstream-slices-runner.XXXXXX")"
RUST_BIN="$RUNNER_TMP_DIR/rust_runner"
RESULTS_TSV="$RUNNER_TMP_DIR/results.tsv"
touch "$RESULTS_TSV"

cleanup_runner() {
    rm -rf "$RUNNER_TMP_DIR"
}
trap cleanup_runner EXIT

if ! rustc --edition=2024 "$RUST_RUNNER_SRC" -L dependency="$ROOT_DIR/target/debug/deps" --extern rmath="$RUST_RLIB" -o "$RUST_BIN" >"$RUNNER_TMP_DIR/rustc.log" 2>&1; then
    echo "ERROR: failed to compile Rust upstream slice runner"
    sed 's/^/  rustc | /' "$RUNNER_TMP_DIR/rustc.log"
    exit 1
fi

normalize_output() {
    tr -d '\r' |
        sed 's/[[:space:]]*$//' |
        awk '{ lines[NR] = $0 } END { n = NR; while (n > 0 && lines[n] == "") n--; for (i = 1; i <= n; i++) print lines[i] }'
}

is_xfail() {
    local case_name="$1"
    [[ -f "$XFAIL_FILE" ]] && awk -F '\t' -v case_name="$case_name" \
        'NF && $1 !~ /^#/ && $1 == case_name { found = 1 } END { exit found ? 0 : 1 }' \
        "$XFAIL_FILE"
}

record_result() {
    local case_name="$1"
    local status="$2"
    local note="${3:-}"
    printf '%s\t%s\t%s\n' "$case_name" "$status" "$note" >>"$RESULTS_TSV"
}

run_case() {
    local case_file="$1"
    local case_name
    case_name="$(basename "$case_file" .R)"

    local tmp_dir
    tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/rport-upstream-slice.XXXXXX")"
    local c_out="$tmp_dir/c.out"
    local r_out="$tmp_dir/r.out"
    local c_norm="$tmp_dir/c.norm"
    local r_norm="$tmp_dir/r.norm"

    if ! env LC_ALL=C LANG=C Rscript --vanilla "$case_file" >"$c_out" 2>&1; then
        echo "FAIL ${case_name}: stock R exited non-zero"
        sed 's/^/  C | /' "$c_out"
        rm -rf "$tmp_dir"
        return 1
    fi

    if ! env LC_ALL=C LANG=C "$RUST_BIN" "$case_file" >"$r_out" 2>&1; then
        echo "FAIL ${case_name}: Rust runner exited non-zero"
        sed 's/^/  R | /' "$r_out"
        rm -rf "$tmp_dir"
        return 1
    fi

    normalize_output <"$c_out" >"$c_norm"
    normalize_output <"$r_out" >"$r_norm"

    if ! cmp -s "$c_norm" "$r_norm"; then
        echo "FAIL ${case_name}: Rust output diverged from stock R"
        diff -u "$c_norm" "$r_norm" || true
        rm -rf "$tmp_dir"
        return 1
    fi

    echo "PASS ${case_name}"
    rm -rf "$tmp_dir"
}

write_report() {
    [[ -n "$REPORT_DIR" ]] || return 0
    mkdir -p "$REPORT_DIR"
    local report="$REPORT_DIR/summary.md"
    {
        echo "# Upstream Core Slice Report"
        echo
        echo "| Case | Status | Note |"
        echo "| --- | --- | --- |"
        awk -F '\t' '{ printf("| `%s` | %s | %s |\n", $1, $2, $3) }' "$RESULTS_TSV"
    } >"$report"
    echo "INFO: wrote upstream slice Markdown report to $report"
}

main() {
    shopt -s nullglob
    local cases=("$CASES_DIR"/*.R)
    shopt -u nullglob

    if (( ${#cases[@]} == 0 )); then
        echo "ERROR: no upstream slice cases found in $CASES_DIR" >&2
        exit 1
    fi

    local total=0
    local passed=0
    local xfailed=0
    local xpassed=0
    local failed=0

    local case_file
    for case_file in "${cases[@]}"; do
        total=$((total + 1))
        local case_name
        case_name="$(basename "$case_file" .R)"
        if run_case "$case_file"; then
            if is_xfail "$case_name"; then
                echo "XPASS ${case_name}: remove from $XFAIL_FILE or fix the owner bead"
                record_result "$case_name" "xpass" "listed in xfail.tsv but now passes"
                xpassed=$((xpassed + 1))
                failed=$((failed + 1))
            else
                record_result "$case_name" "pass"
                passed=$((passed + 1))
            fi
        elif is_xfail "$case_name"; then
            echo "XFAIL ${case_name}"
            record_result "$case_name" "xfail" "listed in xfail.tsv"
            xfailed=$((xfailed + 1))
        else
            record_result "$case_name" "fail"
            failed=$((failed + 1))
        fi
    done

    echo "Summary: ${passed}/${total} upstream slices passed, ${xfailed} expected failures"
    write_report

    if (( failed > 0 || xpassed > 0 )); then
        exit 1
    fi
}

main
