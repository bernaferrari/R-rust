#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASES_DIR="$ROOT_DIR/tests/conformance/cases"
GOLDEN_DIR="$ROOT_DIR/tests/conformance/golden"
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
    local rust_rlibs=("$ROOT_DIR"/target/debug/deps/librmath-*.rlib)
    shopt -u nullglob
    if (( ${#rust_rlibs[@]} > 0 )); then
        found="$(ls -t "${rust_rlibs[@]}" 2>/dev/null | head -n1)"
    fi
    printf '%s' "$found"
}

RUST_RLIB="$(find_rust_rlib)"

if [[ -z "$RUST_RLIB" ]]; then
    echo "INFO: Rust rmath artifact not found; building with cargo." >&2
    (cd "$ROOT_DIR" && cargo build -p rmath >/dev/null)
    RUST_RLIB="$(find_rust_rlib)"
fi

if [[ -z "$RUST_RLIB" ]]; then
    echo "ERROR: Rust rmath artifact still missing after build." >&2
    exit 1
fi

normalize_output() {
    tr -d '\r' | sed 's/[[:space:]]*$//'
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

    local rust_bin="$tmp_dir/rust_runner"
    if ! rustc --edition=2024 "$RUST_RUNNER_SRC" -L dependency="$ROOT_DIR/target/debug/deps" --extern rmath="$RUST_RLIB" -o "$rust_bin" >"$tmp_dir/rustc.log" 2>&1; then
        echo "FAIL ${case_name}: failed to compile Rust runner"
        sed 's/^/  rustc | /' "$tmp_dir/rustc.log"
        rm -rf "$tmp_dir"
        return 1
    fi

    if ! env LC_ALL=C LANG=C "$rust_bin" "$case_file" >"$r_out" 2>&1; then
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

main() {
    local total=0
    local passed=0
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
        if run_case "$case_file"; then
            passed=$((passed + 1))
        else
            failed=$((failed + 1))
        fi
    done

    echo "Summary: ${passed}/${total} cases passed"
    if (( failed > 0 )); then
        exit 1
    fi
}

main "$@"
