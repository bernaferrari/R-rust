#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASES_DIR="$ROOT_DIR/tests/conformance/cases"
GOLDEN_DIR="$ROOT_DIR/tests/conformance/golden"
ERROR_CASES_DIR="$ROOT_DIR/tests/conformance/error_cases"
ERROR_GOLDEN_DIR="$ROOT_DIR/tests/conformance/error_golden"
XFAIL_FILE="$ROOT_DIR/tests/conformance/xfail.tsv"
RUST_RUNNER_SRC="$ROOT_DIR/tests/conformance/src/main.rs"

MODE="${1:---check}"
case "$MODE" in
    --check|check) ;;
    *)
        echo "usage: $0 [--check]" >&2
        exit 2
        ;;
esac

if ! command -v Rscript >/dev/null 2>&1; then
    echo "SKIP: Rscript not found; conformance parity checks require stock C R." >&2
    exit 0
fi

if [[ ! -d "$CASES_DIR" ]]; then
    echo "ERROR: missing cases directory: $CASES_DIR" >&2
    exit 1
fi

if [[ ! -d "$GOLDEN_DIR" ]]; then
    echo "ERROR: missing golden directory: $GOLDEN_DIR" >&2
    exit 1
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

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi

echo "INFO: building Rust rmath artifact for conformance runner." >&2
(cd "$ROOT_DIR" && env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" cargo build -p rmath >/dev/null)

RUST_RLIB="$(find_rust_rlib)"

if [[ -z "$RUST_RLIB" ]]; then
    echo "ERROR: Rust rmath artifact still missing after build." >&2
    exit 1
fi

RUNNER_TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rport-conformance-runner.XXXXXX")"
RUST_BIN="$RUNNER_TMP_DIR/rust_runner"
cleanup_runner() {
    rm -rf "$RUNNER_TMP_DIR"
}
trap cleanup_runner EXIT

if ! rustc --edition=2024 "$RUST_RUNNER_SRC" -L dependency="$ROOT_DIR/target/debug/deps" --extern rmath="$RUST_RLIB" -o "$RUST_BIN" >"$RUNNER_TMP_DIR/rustc.log" 2>&1; then
    echo "ERROR: failed to compile Rust conformance runner"
    sed 's/^/  rustc | /' "$RUNNER_TMP_DIR/rustc.log"
    exit 1
fi

normalize_output() {
    tr -d '\r' |
        sed 's/[[:space:]]*$//' |
        awk '{ lines[NR] = $0 } END { n = NR; while (n > 0 && lines[n] == "") n--; for (i = 1; i <= n; i++) print lines[i] }'
}

normalize_error_output() {
    normalize_output |
        sed -E '/^Error in .* :$/ { N; s/^Error in .* :\n[[:space:]]*/Error: /; }' |
        sed '/^Execution halted$/d' |
        sed -E 's/^Error in .* : /Error: /'
}

is_xfail() {
    local case_name="$1"
    [[ -f "$XFAIL_FILE" ]] && awk -F '\t' -v case_name="$case_name" \
        'NF && $1 !~ /^#/ && $1 == case_name { found = 1 } END { exit found ? 0 : 1 }' \
        "$XFAIL_FILE"
}

run_case() {
    local case_file="$1"
    local case_name
    case_name="$(basename "$case_file" .R)"

    local golden_file="$GOLDEN_DIR/${case_name}.out"
    if [[ ! -f "$golden_file" ]]; then
        echo "FAIL ${case_name}: missing golden file $golden_file"
        return 1
    fi

    local tmp_dir
    tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/rport-conformance.XXXXXX")"

    local c_out="$tmp_dir/c.out"
    local r_out="$tmp_dir/r.out"
    local c_norm="$tmp_dir/c.norm"
    local r_norm="$tmp_dir/r.norm"
    local g_norm="$tmp_dir/golden.norm"

    if ! env LC_ALL=C LANG=C Rscript --vanilla "$case_file" >"$c_out" 2>&1; then
        echo "FAIL ${case_name}: Rscript exited non-zero"
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
    normalize_output <"$golden_file" >"$g_norm"

    if ! cmp -s "$c_norm" "$g_norm"; then
        echo "FAIL ${case_name}: C R output diverged from golden"
        diff -u "$g_norm" "$c_norm" || true
        rm -rf "$tmp_dir"
        return 1
    fi

    if ! cmp -s "$r_norm" "$g_norm"; then
        echo "FAIL ${case_name}: Rust output diverged from golden"
        diff -u "$g_norm" "$r_norm" || true
        rm -rf "$tmp_dir"
        return 1
    fi

    echo "PASS ${case_name}"
    rm -rf "$tmp_dir"
}

run_error_case() {
    local case_file="$1"
    local case_name
    case_name="$(basename "$case_file" .R)"

    local golden_file="$ERROR_GOLDEN_DIR/${case_name}.out"
    if [[ ! -f "$golden_file" ]]; then
        echo "FAIL ${case_name}: missing error golden file $golden_file"
        return 1
    fi

    local tmp_dir
    tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/rport-conformance-error.XXXXXX")"

    local c_out="$tmp_dir/c.out"
    local r_out="$tmp_dir/r.out"
    local c_norm="$tmp_dir/c.norm"
    local r_norm="$tmp_dir/r.norm"
    local g_norm="$tmp_dir/golden.norm"

    if env LC_ALL=C LANG=C Rscript --vanilla "$case_file" >"$c_out" 2>&1; then
        echo "FAIL ${case_name}: Rscript succeeded, expected error"
        sed 's/^/  C | /' "$c_out"
        rm -rf "$tmp_dir"
        return 1
    fi

    if env LC_ALL=C LANG=C "$RUST_BIN" "$case_file" >"$r_out" 2>&1; then
        echo "FAIL ${case_name}: Rust runner succeeded, expected error"
        sed 's/^/  R | /' "$r_out"
        rm -rf "$tmp_dir"
        return 1
    fi

    normalize_error_output <"$c_out" >"$c_norm"
    normalize_error_output <"$r_out" >"$r_norm"
    normalize_error_output <"$golden_file" >"$g_norm"

    if ! cmp -s "$c_norm" "$g_norm"; then
        echo "FAIL ${case_name}: C R error diverged from golden"
        diff -u "$g_norm" "$c_norm" || true
        rm -rf "$tmp_dir"
        return 1
    fi

    if ! cmp -s "$r_norm" "$g_norm"; then
        echo "FAIL ${case_name}: Rust error diverged from golden"
        diff -u "$g_norm" "$r_norm" || true
        rm -rf "$tmp_dir"
        return 1
    fi

    echo "PASS ${case_name}"
    rm -rf "$tmp_dir"
}

main() {
    local total=0
    local passed=0
    local xfailed=0
    local xpassed=0
    local failed=0

    shopt -s nullglob
    local cases=("$CASES_DIR"/*.R)
    shopt -u nullglob

    if (( ${#cases[@]} == 0 )); then
        echo "ERROR: no conformance cases found in $CASES_DIR" >&2
        exit 1
    fi

    local case_file
    for case_file in "${cases[@]}"; do
        total=$((total + 1))
        local case_name
        case_name="$(basename "$case_file" .R)"
        if run_case "$case_file"; then
            if is_xfail "$case_name"; then
                echo "XPASS ${case_name}: remove from $XFAIL_FILE or fix the owner bead"
                xpassed=$((xpassed + 1))
                failed=$((failed + 1))
            else
                passed=$((passed + 1))
            fi
        elif is_xfail "$case_name"; then
            echo "XFAIL ${case_name}"
            xfailed=$((xfailed + 1))
        else
            failed=$((failed + 1))
        fi
    done

    shopt -s nullglob
    local error_cases=("$ERROR_CASES_DIR"/*.R)
    shopt -u nullglob

    for case_file in "${error_cases[@]}"; do
        total=$((total + 1))
        local case_name
        case_name="$(basename "$case_file" .R)"
        if run_error_case "$case_file"; then
            if is_xfail "$case_name"; then
                echo "XPASS ${case_name}: remove from $XFAIL_FILE or fix the owner bead"
                xpassed=$((xpassed + 1))
                failed=$((failed + 1))
            else
                passed=$((passed + 1))
            fi
        elif is_xfail "$case_name"; then
            echo "XFAIL ${case_name}"
            xfailed=$((xfailed + 1))
        else
            failed=$((failed + 1))
        fi
    done

    echo "Summary: ${passed}/${total} cases passed, ${xfailed} expected failures"
    if (( xpassed > 0 )); then
        echo "Unexpected passes: ${xpassed}"
    fi
    if (( failed > 0 )); then
        exit 1
    fi
}

main "$@"
