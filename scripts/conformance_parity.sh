#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASES_DIR="$ROOT_DIR/tests/conformance/cases"
GOLDEN_DIR="$ROOT_DIR/tests/conformance/golden"
ERROR_CASES_DIR="$ROOT_DIR/tests/conformance/error_cases"
ERROR_GOLDEN_DIR="$ROOT_DIR/tests/conformance/error_golden"
XFAIL_FILE="$ROOT_DIR/tests/conformance/xfail.tsv"
RUST_RUNNER_SRC="$ROOT_DIR/tests/conformance/src/main.rs"

MODE="--check"
REPORT_DIR=""
REPORT_JSON=""
REPORT_MD=""
STRICT=0

while (($# > 0)); do
    case "$1" in
        --check|check)
            MODE="--check"
            shift
            ;;
        --strict)
            STRICT=1
            shift
            ;;
        --report)
            if (($# < 2)); then
                echo "usage: $0 [--check] [--strict] [--report DIR] [--json FILE] [--markdown FILE]" >&2
                exit 2
            fi
            REPORT_DIR="$2"
            shift 2
            ;;
        --json)
            if (($# < 2)); then
                echo "usage: $0 [--check] [--strict] [--report DIR] [--json FILE] [--markdown FILE]" >&2
                exit 2
            fi
            REPORT_JSON="$2"
            shift 2
            ;;
        --markdown)
            if (($# < 2)); then
                echo "usage: $0 [--check] [--strict] [--report DIR] [--json FILE] [--markdown FILE]" >&2
                exit 2
            fi
            REPORT_MD="$2"
            shift 2
            ;;
        *)
            echo "usage: $0 [--check] [--strict] [--report DIR] [--json FILE] [--markdown FILE]" >&2
            exit 2
            ;;
    esac
done

if [[ -n "$REPORT_DIR" ]]; then
    mkdir -p "$REPORT_DIR"
    REPORT_JSON="${REPORT_JSON:-$REPORT_DIR/summary.json}"
    REPORT_MD="${REPORT_MD:-$REPORT_DIR/summary.md}"
fi

if [[ -n "$REPORT_JSON" ]]; then
    mkdir -p "$(dirname "$REPORT_JSON")"
fi

if [[ -n "$REPORT_MD" ]]; then
    mkdir -p "$(dirname "$REPORT_MD")"
fi

if ! command -v Rscript >/dev/null 2>&1; then
    if [[ "$STRICT" -eq 1 ]]; then
        echo "ERROR: Rscript not found; strict conformance parity requires stock GNU R." >&2
        exit 1
    else
        echo "SKIP: Rscript not found; conformance parity checks require stock C R." >&2
        exit 0
    fi
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

RESULTS_TSV="$RUNNER_TMP_DIR/results.tsv"
touch "$RESULTS_TSV"

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
        sed '/^Calls:/d' |
        sed '/^Execution halted$/d' |
        sed -E 's/^Error in .* : /Error: /'
}

is_xfail() {
    local case_name="$1"
    [[ -f "$XFAIL_FILE" ]] && awk -F '\t' -v case_name="$case_name" \
        'NF && $1 !~ /^#/ && $1 == case_name { found = 1 } END { exit found ? 0 : 1 }' \
        "$XFAIL_FILE"
}

record_result() {
    local case_name="$1"
    local kind="$2"
    local status="$3"
    local detail="${4:-}"
    printf '%s\t%s\t%s\t%s\n' "$case_name" "$kind" "$status" "$detail" >>"$RESULTS_TSV"
}

write_report() {
    if [[ -z "$REPORT_JSON" && -z "$REPORT_MD" ]]; then
        return 0
    fi

    if ! command -v python3 >/dev/null 2>&1; then
        echo "ERROR: python3 is required when --report/--json/--markdown is used." >&2
        return 1
    fi

    python3 - "$RESULTS_TSV" "${REPORT_JSON:-}" "${REPORT_MD:-}" "$XFAIL_FILE" <<'PY'
import csv
import datetime as dt
import json
import pathlib
import sys

results_path = pathlib.Path(sys.argv[1])
json_path = pathlib.Path(sys.argv[2]) if sys.argv[2] else None
markdown_path = pathlib.Path(sys.argv[3]) if sys.argv[3] else None
xfail_path = pathlib.Path(sys.argv[4])

STATUS_ORDER = ("pass", "fail", "xfail", "xpass")
DOMAIN_ORDER = (
    "Parser and scalar basics",
    "Evaluator, closures, and control flow",
    "Vectors, lists, attributes, and objects",
    "Base functions, conditions, and platform helpers",
    "Stats, math, and RNG",
    "Packages, namespaces, and S3",
    "Graphics and Android embedding",
    "Error semantics",
)

def domain_for(case: str, kind: str) -> str:
    if kind == "error":
        return "Error semantics"
    name = case[4:] if len(case) > 4 and case[:3].isdigit() else case
    number = int(case[:3]) if len(case) >= 3 and case[:3].isdigit() else None
    if any(token in name for token in (
        "library", "package", "namespace", "S3", "s3",
    )):
        return "Packages, namespaces, and S3"
    if any(token in name for token in (
        "plot", "graphics", "android", "render",
    )):
        return "Graphics and Android embedding"
    if number is not None:
        if 1 <= number <= 10 or 40 <= number <= 41:
            return "Parser and scalar basics"
        if 11 <= number <= 20:
            return "Evaluator, closures, and control flow"
        if 22 <= number <= 32 or 42 <= number <= 50:
            return "Vectors, lists, attributes, and objects"
        if 33 <= number <= 39 or 82 <= number <= 126:
            return "Stats, math, and RNG"
        if 51 <= number <= 81 or 127 <= number <= 160:
            return "Base functions, conditions, and platform helpers"
    if any(token in name for token in (
        "dnorm", "pnorm", "qnorm", "dbinom", "pbinom", "dpois", "ppois",
        "dgamma", "pgamma", "qgamma", "dbeta", "pbeta", "qbeta", "dcauchy",
        "pcauchy", "qcauchy", "dt_", "pt_", "qt_", "dchisq", "pchisq",
        "qchisq", "dweibull", "pweibull", "qweibull", "df_", "pf_",
        "qf_", "dnbinom", "pnbinom", "qnbinom", "dgeom", "pgeom",
        "qgeom", "dexp", "pexp", "qexp", "sample", "scalar_math", "mean",
        "sum", "range", "min", "cumsum", "cumprod", "diff",
    )):
        return "Stats, math, and RNG"
    if any(token in name for token in (
        "closure", "missing_arg", "while", "control_flow", "assignment_invisible",
        "infix_newline",
    )):
        return "Evaluator, closures, and control flow"
    if any(token in name for token in (
        "vector", "subset", "list", "names", "factor", "matrix", "class",
        "data_frame", "inherits", "raw", "toString", "setNames",
    )):
        return "Vectors, lists, attributes, and objects"
    if any(token in name for token in (
        "file", "tempdir", "assign", "get", "exists", "rm", "cat", "print",
        "capture", "warning", "message", "tryCatch", "regexpr", "proc_time",
        "system", "ls_", "is_primitive", "is_loaded", "is_unsorted", "sort",
        "unique", "match", "union", "intersect", "setdiff", "setequal",
        "which_", "any", "all", "seq_",
    )):
        return "Base functions, conditions, and platform helpers"
    return "Parser and scalar basics"

def empty_counts():
    return {status: 0 for status in STATUS_ORDER}

rows = []
with results_path.open(newline="") as fh:
    reader = csv.reader(fh, delimiter="\t")
    for row in reader:
        if not row:
            continue
        case, kind, status, detail = (row + ["", "", "", ""])[:4]
        rows.append({
            "case": case,
            "kind": kind,
            "status": status,
            "detail": detail,
            "domain": domain_for(case, kind),
        })

totals = empty_counts()
domains = {domain: {"domain": domain, **empty_counts(), "cases": []} for domain in DOMAIN_ORDER}
for row in rows:
    totals[row["status"]] = totals.get(row["status"], 0) + 1
    domain = domains.setdefault(row["domain"], {"domain": row["domain"], **empty_counts(), "cases": []})
    domain[row["status"]] = domain.get(row["status"], 0) + 1
    domain["cases"].append(row)

for domain in domains.values():
    domain["total"] = sum(domain.get(status, 0) for status in STATUS_ORDER)

xfails = []
if xfail_path.exists():
    with xfail_path.open() as fh:
        for line in fh:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            parts = line.split("\t")
            xfails.append({
                "case": parts[0] if len(parts) > 0 else "",
                "owner": parts[1] if len(parts) > 1 else "",
                "reason": parts[2] if len(parts) > 2 else "",
            })

report = {
    "generated_at_utc": dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat(),
    "total": len(rows),
    "passed": totals.get("pass", 0),
    "failed": totals.get("fail", 0),
    "expected_failures": totals.get("xfail", 0),
    "unexpected_passes": totals.get("xpass", 0),
    "status_counts": totals,
    "domains": list(domains.values()),
    "xfails": xfails,
}

if json_path:
    json_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

if markdown_path:
    lines = [
        "# rport Conformance Report",
        "",
        f"Generated: `{report['generated_at_utc']}`",
        "",
        "## Summary",
        "",
        "| Metric | Count |",
        "| --- | ---: |",
        f"| Total cases | {report['total']} |",
        f"| Passing | {report['passed']} |",
        f"| Failing | {report['failed']} |",
        f"| Expected failures | {report['expected_failures']} |",
        f"| Unexpected passes | {report['unexpected_passes']} |",
        "",
        "## Domains",
        "",
        "| Domain | Pass | Fail | XFail | XPass | Total |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for domain in report["domains"]:
        lines.append(
            f"| {domain['domain']} | {domain.get('pass', 0)} | {domain.get('fail', 0)} | "
            f"{domain.get('xfail', 0)} | {domain.get('xpass', 0)} | {domain['total']} |"
        )
    failing = [row for row in rows if row["status"] in {"fail", "xfail", "xpass"}]
    lines.extend(["", "## Non-Passing Cases", ""])
    if failing:
        lines.extend(["| Case | Domain | Status | Detail |", "| --- | --- | --- | --- |"])
        for row in failing:
            detail = row["detail"].replace("|", "\\|")
            lines.append(f"| `{row['case']}` | {row['domain']} | {row['status']} | {detail} |")
    else:
        lines.append("None.")
    lines.extend([
        "",
        "## Policy",
        "",
        "- `pass`: stock C R, checked-in golden output, and the Rust runtime agree after deterministic normalization.",
        "- `fail`: behavior differs and must be fixed or moved to `tests/conformance/xfail.tsv` with an owner bead.",
        "- `xfail`: known accepted gap with an owner bead.",
        "- `xpass`: behavior now passes despite being listed as expected-fail; remove the stale xfail entry.",
        "",
    ])
    markdown_path.write_text("\n".join(lines))
PY

    if [[ -n "$REPORT_JSON" ]]; then
        echo "INFO: wrote conformance JSON report to $REPORT_JSON"
    fi
    if [[ -n "$REPORT_MD" ]]; then
        echo "INFO: wrote conformance Markdown report to $REPORT_MD"
    fi
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
                record_result "$case_name" "normal" "xpass" "listed in xfail.tsv but now passes"
                xpassed=$((xpassed + 1))
                failed=$((failed + 1))
            else
                record_result "$case_name" "normal" "pass"
                passed=$((passed + 1))
            fi
        elif is_xfail "$case_name"; then
            echo "XFAIL ${case_name}"
            record_result "$case_name" "normal" "xfail" "listed in xfail.tsv"
            xfailed=$((xfailed + 1))
        else
            record_result "$case_name" "normal" "fail"
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
                record_result "$case_name" "error" "xpass" "listed in xfail.tsv but now passes"
                xpassed=$((xpassed + 1))
                failed=$((failed + 1))
            else
                record_result "$case_name" "error" "pass"
                passed=$((passed + 1))
            fi
        elif is_xfail "$case_name"; then
            echo "XFAIL ${case_name}"
            record_result "$case_name" "error" "xfail" "listed in xfail.tsv"
            xfailed=$((xfailed + 1))
        else
            record_result "$case_name" "error" "fail"
            failed=$((failed + 1))
        fi
    done

    echo "Summary: ${passed}/${total} cases passed, ${xfailed} expected failures"
    if (( xpassed > 0 )); then
        echo "Unexpected passes: ${xpassed}"
    fi
    if [[ "$STRICT" -eq 1 && "$xfailed" -gt 0 ]]; then
        echo "Strict mode: ${xfailed} expected failures remain; fix them or remove --strict."
        failed=$((failed + xfailed))
    fi
    write_report
    if (( failed > 0 )); then
        exit 1
    fi
}

main "$@"
