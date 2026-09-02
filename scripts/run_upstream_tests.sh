#!/usr/bin/env bash
# Run upstream R regression tests (vendored r-source/tests/*.R) UNMODIFIED
# against the rport Rust interpreter and diff the behavior against real R:
#
#   trunk R  (default /tmp/r-trunk/bin)          -- primary oracle
#   stock R  (default /opt/homebrew/Cellar/r/4.6.1/bin) -- secondary oracle
#
# A file PASSES when rport's stdout matches trunk R's stdout (modulo trailing
# whitespace) AND both agree on the error class (both clean or both abort).
# stderr wording is recorded but not required to match (R messages are
# locale/version specific).
#
# Usage:
#   scripts/run_upstream_tests.sh                 # curated core-semantics list
#   scripts/run_upstream_tests.sh arith.R seq.R   # explicit files
#   scripts/run_upstream_tests.sh --all           # every r-source/tests/*.R
#
# Environment:
#   RPORT_TRUNK_R            dir holding Rscript (default /tmp/r-trunk/bin)
#   RPORT_STOCK_R            dir holding Rscript (default homebrew 4.6.1)
#   RPORT_UPSTREAM_TIMEOUT   per-file seconds (default 120)
#   RPORT_UPSTREAM_REPORT    report dir (default target/upstream-report)
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TESTS_DIR="$ROOT_DIR/r-source/tests"
HELPER_CRATE="$ROOT_DIR/tests/upstream-run"
REPORT_DIR="${RPORT_UPSTREAM_REPORT:-$ROOT_DIR/target/upstream-report}"
TRUNK_BIN="${RPORT_TRUNK_R:-/tmp/r-trunk/bin}"
STOCK_BIN="${RPORT_STOCK_R:-/opt/homebrew/Cellar/r/4.6.1/bin}"
TIMEOUT_SECS="${RPORT_UPSTREAM_TIMEOUT:-120}"

# Core-semantics files most likely to pass; the task's arithmetic/seq/logic/
# character trio maps onto arith(.R/-true.R), any-all, complex, structure, ...
DEFAULT_FILES=(
    arith.R
    arith-true.R
    any-all.R
    complex.R
    conditions.R
    structure.R
    simple-true.R
    eval-etc.R
    eval-etc-2.R
    ok-errors.R
    primitives.R
    array-subset.R
    datetime.R
)

FILES=()
ALL=0
while (($# > 0)); do
    case "$1" in
    --all)
        ALL=1
        ;;
    -h | --help)
        sed -n '2,25p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        FILES+=("$1")
        ;;
    esac
    shift
done

if [[ ! -d "$TESTS_DIR" ]]; then
    echo "ERROR: vendored R tests not found at $TESTS_DIR" >&2
    echo "       run scripts/fetch-r-source.sh first" >&2
    exit 2
fi
if [[ ! -x "$TRUNK_BIN/Rscript" ]]; then
    echo "ERROR: trunk Rscript not found at $TRUNK_BIN/Rscript" >&2
    exit 2
fi
if [[ ! -x "$STOCK_BIN/Rscript" ]]; then
    echo "WARN: stock Rscript not found at $STOCK_BIN/Rscript; secondary oracle disabled" >&2
    STOCK_BIN=""
fi
if ! command -v timeout >/dev/null 2>&1; then
    echo "ERROR: timeout(1) is required" >&2
    exit 2
fi

# Build the rport file-runner helper (standalone crate; shares nothing with
# the root workspace so this never collides with sibling builds).
HELPER_BIN="$HELPER_CRATE/target/release/rport-upstream-run"
if [[ ! -x "$HELPER_BIN" ]] || [[ -n "$(find "$HELPER_CRATE/src" "$HELPER_CRATE/Cargo.toml" -newer "$HELPER_BIN" 2>/dev/null)" ]]; then
    echo "INFO: building rport upstream helper (release)..." >&2
    (cd "$HELPER_CRATE" && cargo build --release --offline >/dev/null) ||
        (cd "$HELPER_CRATE" && cargo build --release >/dev/null)
fi

mkdir -p "$REPORT_DIR"
SUMMARY_TSV="$REPORT_DIR/summary.tsv"
SUMMARY_MD="$REPORT_DIR/summary.md"
: >"$SUMMARY_TSV"
printf 'file\tverdict\treason\ttrunk_exit\trport_exit\tstock_exit\tstdout=trunk\tstdout=stock\tstderr=trunk\n' >"$SUMMARY_TSV"

# normalize <file> : strip CRs, trailing whitespace, trailing blank lines.
normalize() {
    tr -d '\r' <"$1" | sed -e 's/[[:space:]]\+$//' -e :a -e '/^$/{$d;N;ba' -e '}'
}

first_error_line() { # best-effort single-line R error/warning extract
    iconv -c -f UTF-8 -t UTF-8 "$1" 2>/dev/null | grep -m1 -E '^(Error|Warning|error|Fehler|Execution halted|panicked at)' |
        cut -c1-160 || true
}

diff_excerpt() { # first hunk of unified diff, capped, sanitized
    iconv -c -f UTF-8 -t UTF-8 "$1" 2>/dev/null | sed -n '/^@@/,$p' | sed -n '1,16p'
}
classify_exit() { # classify_exit <exit-code>
    case "$1" in
    0) echo clean ;;
    124) echo timeout ;;
    101) echo panic ;;
    126 | 127 | 2) echo harness-error ;;
    *)
        if (($1 >= 128)); then echo crash-signal; else echo r-error; fi
        ;;
    esac
}

run_engine() { # run_engine <tag> <file.R> <engine-cmd...>; writes <tag>.out/.err
    local tag="$1"
    local file="$2"
    shift 2
    local out="$WORK/${STEM}.${tag}.out"
    local err="$WORK/${STEM}.${tag}.err"
    : >"$out"
    : >"$err"
    local code=0
    (cd "$TESTS_DIR" && timeout "$TIMEOUT_SECS" "$@" "$file" >"$out" 2>"$err") || code=$?
    echo "$code"
}

declare -a RESULTS=()
PASS=0
FAIL=0
CATALOG_LINES=()

run_file() {
    local file="$1"
    local stem="${file%.R}"
    STEM="$stem"

    run_engine trunk "$file" "$TRUNK_BIN/Rscript" --vanilla >"$WORK/${stem}.trunk.exit"
    run_engine rport "$file" "$HELPER_BIN" >"$WORK/${stem}.rport.exit"
    local stock_exit="n/a"
    if [[ -n "$STOCK_BIN" ]]; then
        run_engine stock "$file" "$STOCK_BIN/Rscript" --vanilla >"$WORK/${stem}.stock.exit"
        stock_exit="$(cat "$WORK/${stem}.stock.exit")"
    fi

    normalize "$WORK/${stem}.trunk.out" >"$WORK/${stem}.trunk.norm"
    normalize "$WORK/${stem}.rport.out" >"$WORK/${stem}.rport.norm"

    local trunk_exit rport_exit
    trunk_exit="$(cat "$WORK/${stem}.trunk.exit")"
    rport_exit="$(cat "$WORK/${stem}.rport.exit")"

    local stdout_match=NO stock_match=n/a stderr_note=""
    if cmp -s "$WORK/${stem}.trunk.norm" "$WORK/${stem}.rport.norm"; then
        stdout_match=YES
    else
        diff -u "$WORK/${stem}.trunk.norm" "$WORK/${stem}.rport.norm" >"$WORK/${stem}.diff" || true
    fi
    if [[ -n "$STOCK_BIN" ]]; then
        normalize "$WORK/${stem}.stock.out" >"$WORK/${stem}.stock.norm"
        if cmp -s "$WORK/${stem}.trunk.norm" "$WORK/${stem}.stock.norm"; then
            stock_match=YES
        else
            stock_match=NO
        fi
        if cmp -s "$WORK/${stem}.stock.norm" "$WORK/${stem}.rport.norm"; then
            stock_match="${stock_match}+rport-agrees-with-stock"
        fi
    fi
    if ! cmp -s "$WORK/${stem}.trunk.err" "$WORK/${stem}.rport.err"; then
        stderr_note=differs
    fi

    local trunk_class rport_class verdict reason
    trunk_class="$(classify_exit "$trunk_exit")"
    rport_class="$(classify_exit "$rport_exit")"

    if [[ "$rport_class" == timeout ]]; then
        verdict=FAIL reason=rport-timeout
    elif [[ "$rport_class" == crash-signal || "$rport_class" == panic ]]; then
        verdict=FAIL reason=rport-crash
    elif [[ "$stdout_match" == YES && "$trunk_class" == "$rport_class" ]]; then
        verdict=PASS reason="-"
    elif [[ "$stdout_match" == YES ]]; then
        verdict=FAIL reason=exit-class-mismatch
    elif [[ "$rport_class" != clean && "$trunk_class" == clean ]]; then
        verdict=FAIL reason=rport-error
    elif [[ "$trunk_class" != clean && "$rport_class" == clean ]]; then
        verdict=FAIL reason=rport-completed-where-trunk-errored
    else
        verdict=FAIL reason=output-mismatch
    fi


    if [[ "$verdict" == PASS ]]; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
    fi

    local err_first="-"
    [[ -s "$WORK/${stem}.rport.err" ]] && err_first="$(first_error_line "$WORK/${stem}.rport.err")"
    [[ -z "$err_first" ]] && err_first="-"

    RESULTS+=("$(printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' "$file" "$verdict" "$reason" "$trunk_exit" "$rport_exit" "$stock_exit" "$stdout_match" "$stock_match" "$stderr_note")")

    # Per-failure catalog entry
    local catalog=""
    catalog+="### $file — $reason (trunk exit=$trunk_exit [$trunk_class], rport exit=$rport_exit [$rport_class])"$'\n'
    if [[ -s "$WORK/${stem}.rport.err" ]]; then
        catalog+="rport stderr head: $(first_error_line "$WORK/${stem}.rport.err")"$'\n'
    fi
    if [[ -f "$WORK/${stem}.diff" ]]; then
        catalog+="first stdout diff hunk (trunk vs rport):"$'\n'
        catalog+="$(diff_excerpt "$WORK/${stem}.diff")"$'\n'
    fi
    CATALOG_LINES+=("$catalog")

    printf '%-22s %-8s %-24s trunk_exit=%-3s rport_exit=%-3s stdout=trunk:%-3s stdout=stock:%s\n' \
        "$file" "$verdict" "$reason" "$trunk_exit" "$rport_exit" "$stdout_match" "$stock_match"
}

WORK="$REPORT_DIR/raw"
rm -rf "$WORK"
mkdir -p "$WORK"

if ((ALL)); then
    for path in "$TESTS_DIR"/*.R; do
        FILES+=("$(basename "$path")")
    done
fi
if [[ ${#FILES[@]} -eq 0 ]]; then
    FILES=("${DEFAULT_FILES[@]}")
fi

echo "rport upstream differential runner"
echo "  tests dir : $TESTS_DIR"
echo "  trunk R   : $TRUNK_BIN ($("$TRUNK_BIN/Rscript" --vanilla -e 'cat(R.version.string)' 2>/dev/null))"
if [[ -n "$STOCK_BIN" ]]; then
    echo "  stock R   : $STOCK_BIN ($("$STOCK_BIN/Rscript" --vanilla -e 'cat(R.version.string)' 2>/dev/null))"
fi
echo "  rport     : $HELPER_BIN"
echo "  files     : ${#FILES[@]}  timeout: ${TIMEOUT_SECS}s"
echo

for file in "${FILES[@]}"; do
    if [[ ! -f "$TESTS_DIR/$file" ]]; then
        echo "SKIP $file: not found in $TESTS_DIR" >&2
        continue
    fi
    run_file "$file"
done

TOTAL=$((PASS + FAIL))

{
    printf '\n'
    printf '== PASS %d / %d (%d%%) — FAIL %d ==\n' "$PASS" "$TOTAL" "$((TOTAL > 0 ? PASS * 100 / TOTAL : 0))" "$FAIL"
} | tee -a "$REPORT_DIR/pass_rate.txt"

# Assemble the markdown report with the failure catalog.
{
    echo "# Upstream R differential report"
    echo
    echo "- date: $(date -u +%FT%TZ)"
    echo "- trunk: $TRUNK_BIN | stock: ${STOCK_BIN:-disabled} | timeout: ${TIMEOUT_SECS}s"
    echo "- pass rate: $PASS / $TOTAL"
    echo
    echo "| file | verdict | reason | trunk_exit | rport_exit | stock_exit | stdout=trunk | stdout=stock | stderr=trunk |"
    echo "|------|---------|--------|-----------:|-----------:|-----------:|--------------|--------------|--------------|"
    printf '%s\n' "${RESULTS[@]}" >>"$SUMMARY_TSV"
    tail -n +2 "$SUMMARY_TSV" | awk -F'\t' '{print "| "$1" | "$2" | "$3" | "$4" | "$5" | "$6" | "$7" | "$8" | "$9" |"}'
    echo
    echo "## Failure catalog"
    echo
    if ((FAIL == 0)); then
        echo "(none)"
    else
        printf '%s\n' "${CATALOG_LINES[@]}"
    fi
} >"$SUMMARY_MD"

echo
echo "report: $SUMMARY_MD"
echo "table: $SUMMARY_TSV"

if ((FAIL > 0)); then
    exit 1
fi
