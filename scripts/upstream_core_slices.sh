#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASES_DIR="$ROOT_DIR/tests/upstream-core/cases"
XFAIL_FILE="$ROOT_DIR/tests/upstream-core/xfail.tsv"
UPSTREAM_CORPUS_DIR="$ROOT_DIR/tests/upstream-r"
UPSTREAM_VENDOR_DIR="$UPSTREAM_CORPUS_DIR/vendor"
UPSTREAM_DISPOSITIONS="$UPSTREAM_CORPUS_DIR/dispositions.tsv"
RUST_RUNNER_SRC="$ROOT_DIR/tests/conformance/src/main.rs"
REPORT_DIR=""
STRICT=0

usage() {
    cat <<'USAGE'
Usage: scripts/upstream_core_slices.sh [--strict] [--report DIR]

Validates the complete pinned top-level r-source/tests/*.R inventory, then runs
every pass/xfail whole-file disposition and the curated supported slices against
stock R and the Rust runtime.
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
        --strict)
            STRICT=1
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

if [[ -n "$REPORT_DIR" ]]; then
    python3 "$ROOT_DIR/scripts/validate_upstream_r_tests.py" \
        --markdown "$REPORT_DIR/upstream-inventory.md"
else
    python3 "$ROOT_DIR/scripts/validate_upstream_r_tests.py"
fi

if ! command -v Rscript >/dev/null 2>&1; then
    if [[ "$STRICT" -eq 1 ]]; then
        echo "ERROR: Rscript not found; strict upstream parity requires stock GNU R." >&2
        exit 1
    else
        echo "SKIP: Rscript not found; upstream parity requires stock GNU R." >&2
        exit 0
    fi
fi

if [[ "${RPORT_REQUIRE_PINNED_ORACLE:-0}" == "1" ]]; then
    python3 "$ROOT_DIR/scripts/validate_r_oracle.py" \
        --runtime "$(command -v Rscript)"
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

    local case_dir case_basename
    case_dir="$(dirname "$case_file")"
    case_basename="$(basename "$case_file")"

    if ! (
        cd "$case_dir" &&
            env LC_ALL=C LANG=C SRCDIR="$case_dir" \
                Rscript --vanilla "$case_basename"
    ) >"$c_out" 2>&1; then
        echo "FAIL ${case_name}: stock R exited non-zero"
        sed 's/^/  C | /' "$c_out"
        rm -rf "$tmp_dir"
        return 1
    fi

    if ! (
        cd "$case_dir" &&
            env LC_ALL=C LANG=C SRCDIR="$case_dir" \
                "$RUST_BIN" "$case_basename"
    ) >"$r_out" 2>&1; then
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
        echo "# GNU R Upstream Parity Report"
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
    local skipped=0
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

    while IFS=$'\t' read -r upstream_path disposition owner reason; do
        [[ -n "$upstream_path" && "$upstream_path" != \#* ]] || continue
        total=$((total + 1))
        case_file="$UPSTREAM_VENDOR_DIR/$upstream_path"
        case_name="upstream/$upstream_path"
        case "$disposition" in
            skip)
                echo "SKIP ${case_name}: ${reason} (${owner})"
                record_result "$case_name" "skip" "${reason} (${owner})"
                skipped=$((skipped + 1))
                ;;
            pass)
                if run_case "$case_file"; then
                    record_result "$case_name" "pass"
                    passed=$((passed + 1))
                else
                    record_result "$case_name" "fail" "declared pass"
                    failed=$((failed + 1))
                fi
                ;;
            xfail)
                if run_case "$case_file"; then
                    echo "XPASS ${case_name}: remove its xfail or close ${owner}"
                    record_result "$case_name" "xpass" "${reason} (${owner})"
                    xpassed=$((xpassed + 1))
                    failed=$((failed + 1))
                else
                    echo "XFAIL ${case_name}: ${reason} (${owner})"
                    record_result "$case_name" "xfail" "${reason} (${owner})"
                    xfailed=$((xfailed + 1))
                fi
                ;;
        esac
    done <"$UPSTREAM_DISPOSITIONS"

    echo "Summary: ${passed}/${total} upstream cases passed, ${xfailed} expected failures, ${skipped} skipped"
    write_report

    if (( failed > 0 || xpassed > 0 )); then
        exit 1
    fi
}

main
