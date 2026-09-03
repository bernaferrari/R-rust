#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_DIR="$ROOT_DIR/target/compatibility-report"
CHECK=0

usage() {
    cat <<'USAGE'
Usage: scripts/compat_report.sh [OPTIONS]

Writes a machine-readable compatibility snapshot to REPORT_DIR/
(compatibility-report.json + compatibility-report.md by default under
target/). Facts are gathered statically — no oracle build, no test run:

  pinned R commit (tests/upstream-r/inventory.tsv header),
  toolchain (rustc --version),
  conformance case counts by directory,
  upstream slice inventory (attempted vs xfail),
  synthetic package matrix size (crates/r-embed/tests/embed.rs),
  unsafe-token line count (grep over *.rs),
  miri / gc-torture nightly job status lines.

Options:
  --check          Fail when any probe fails (missing pin, empty corpus, ...).
  --report DIR     Write summary artifacts to DIR.
  -h, --help       Show this help.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --check)
            CHECK=1
            shift
            ;;
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

cd "$ROOT_DIR"
mkdir -p "$REPORT_DIR"

status=0
fail() { echo "PROBE-FAIL: $1" >&2; status=1; }

# Pinned R commit from the inventory header.
PINNED_COMMIT="$(awk -F'\t' '/^# r-source commit/ { print $2; exit }' tests/upstream-r/inventory.tsv 2>/dev/null || true)"
[[ -n "${PINNED_COMMIT:-}" ]] || fail "no pinned r-source commit in tests/upstream-r/inventory.tsv"
ARCHIVE_SHA="$(awk -F'\t' '/^# archive sha256/ { print $2; exit }' tests/upstream-r/inventory.tsv 2>/dev/null || true)"

# Toolchain.
RUSTC_VERSION="$(rustc --version 2>/dev/null || true)"
[[ -n "${RUSTC_VERSION:-}" ]] || fail "rustc not on PATH"
CARGO_VERSION="$(cargo --version 2>/dev/null || true)"

# Conformance case counts by directory.
count_r() { ls "$1" 2>/dev/null | grep -c '\.R$' || true; }
CASES="$(count_r tests/conformance/cases)"
GOLDEN_COUNT="$(ls tests/conformance/golden 2>/dev/null | wc -l | tr -d ' ')"
ERROR_CASES="$(count_r tests/conformance/error_cases)"
ERROR_GOLDEN="$(ls tests/conformance/error_golden 2>/dev/null | wc -l | tr -d ' ')"
TOTAL_R=$((CASES + ERROR_CASES))
[[ "$TOTAL_R" -gt 0 ]] || fail "conformance corpus is empty"
[[ "$CASES" == "$GOLDEN_COUNT" ]] || fail "cases ($CASES) != golden ($GOLDEN_COUNT)"

# Upstream slices: attempted = slice cases on disk; xfail = rows in xfail.tsv.
SLICES_ATTEMPTED="$(count_r tests/upstream-core/cases)"
SLICES_XFAIL="$(( $(wc -l < tests/upstream-core/xfail.tsv 2>/dev/null | tr -d ' ') - 1 ))"
[[ "$SLICES_XFAIL" -ge 0 ]] 2>/dev/null || SLICES_XFAIL=0
SLICES_PASSED=$((SLICES_ATTEMPTED - SLICES_XFAIL))
# Synthetic package feature matrix size (indented literals only; the const
# type annotation `&[SyntheticPkgEntry]` starts at column 0).
MATRIX_COUNT="$(grep -c '^    SyntheticPkgEntry {' crates/r-embed/tests/embed.rs || true)"
if command -v Rscript >/dev/null 2>&1; then RSCRIPT_STATUS="present ($(command -v Rscript))"; else RSCRIPT_STATUS="absent (slices need stock R)"; fi
[[ "$MATRIX_COUNT" == "25" ]] || fail "synthetic matrix holds $MATRIX_COUNT entries, want 25"

# Unsafe-token line count (grep, whole workspace Rust sources).
UNSAFE_LINES="$(grep -rh --include='*.rs' 'unsafe' rmath-rs crates 2>/dev/null | wc -l | tr -d ' ')"

# Miri / GC-torture nightly status (workflow-declared, not executed here).
if grep -q 'cargo +nightly miri test -p rmath sexp::' .github/workflows/nightly.yml 2>/dev/null; then
    MIRI_STATUS="configured (nightly: cargo +nightly miri test -p rmath sexp::)"
else
    MIRI_STATUS="NOT-CONFIGURED"; fail "miri nightly job missing"
fi
if [[ -x scripts/gc_torture_stress.sh ]]; then
    GCTORTURE_STATUS="configured (nightly: scripts/gc_torture_stress.sh)"
else
    GCTORTURE_STATUS="NOT-CONFIGURED"; fail "gc-torture script missing"
fi

export PINNED_COMMIT ARCHIVE_SHA RUSTC_VERSION CARGO_VERSION CASES GOLDEN_COUNT
export ERROR_CASES ERROR_GOLDEN TOTAL_R SLICES_ATTEMPTED SLICES_XFAIL SLICES_PASSED
export RSCRIPT_STATUS MATRIX_COUNT UNSAFE_LINES MIRI_STATUS GCTORTURE_STATUS
export REPORT_DIR STATUS="$status"

python3 - <<'PY'
import datetime as dt
import json
import os
import pathlib

report_dir = pathlib.Path(os.environ["REPORT_DIR"])
status = int(os.environ["STATUS"])

report = {
    "generated_at_utc": dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat(),
    "status": "pass" if status == 0 else "fail",
    "pinned_r_commit": os.environ.get("PINNED_COMMIT") or None,
    "archive_sha256": os.environ.get("ARCHIVE_SHA") or None,
    "toolchain": {
        "rustc": os.environ.get("RUSTC_VERSION") or None,
        "cargo": os.environ.get("CARGO_VERSION") or None,
    },
    "conformance_cases": {
        "cases": int(os.environ["CASES"]),
        "golden": int(os.environ["GOLDEN_COUNT"]),
        "error_cases": int(os.environ["ERROR_CASES"]),
        "error_golden": int(os.environ["ERROR_GOLDEN"]),
        "total_r": int(os.environ["TOTAL_R"]),
    },
    "upstream_slices": {
        "attempted": int(os.environ["SLICES_ATTEMPTED"]),
        "passed": int(os.environ["SLICES_PASSED"]),
        "xfail": int(os.environ["SLICES_XFAIL"]),
        "rscript": os.environ["RSCRIPT_STATUS"],
        "note": "static inventory; live pass/fail needs scripts/upstream_core_slices.sh --strict against the pinned oracle",
    },
    "synthetic_package_feature_matrix": int(os.environ["MATRIX_COUNT"]),
    "unsafe_matching_lines": int(os.environ["UNSAFE_LINES"]),
    "unsafe_method": "grep -rh --include='*.rs' 'unsafe' rmath-rs crates | wc -l",
    "miri": os.environ["MIRI_STATUS"],
    "gc_torture": os.environ["GCTORTURE_STATUS"],
}

(report_dir / "compatibility-report.json").write_text(json.dumps(report, indent=2) + "\n")

lines = [
    "# Compatibility Report",
    "",
    f"Generated: `{report['generated_at_utc']}`",
    "",
    f"Status: **{report['status']}**",
    "",
    "## Pin & Toolchain",
    "",
    f"- Pinned R commit: `{report['pinned_r_commit']}`",
    f"- Archive SHA-256: `{report['archive_sha256']}`",
    f"- Toolchain: `{report['toolchain']['rustc']}` / `{report['toolchain']['cargo']}`",
    "",
    "## Conformance Cases (by directory)",
    "",
    f"- `tests/conformance/cases`: {report['conformance_cases']['cases']}",
    f"- `tests/conformance/golden`: {report['conformance_cases']['golden']}",
    f"- `tests/conformance/error_cases`: {report['conformance_cases']['error_cases']}",
    f"- `tests/conformance/error_golden`: {report['conformance_cases']['error_golden']}",
    f"- Total `.R` fixtures: **{report['conformance_cases']['total_r']}**",
    "",
    "## Upstream Slices",
    "",
    f"- Attempted: {report['upstream_slices']['attempted']}, "
    f"passed: {report['upstream_slices']['passed']}, "
    f"xfail: {report['upstream_slices']['xfail']}",
    f"- Rscript: {report['upstream_slices']['rscript']}",
    f"- _{report['upstream_slices']['note']}_",
    "",
    "## Synthetic Package Feature Matrix",
    "",
    f"- Entries: **{report['synthetic_package_feature_matrix']}** "
    "(`SYNTHETIC_PACKAGE_FEATURE_MATRIX` in `crates/r-embed/tests/embed.rs`)",
    "",
    "## Unsafe Surface (grep)",
    "",
    f"- Lines matching `unsafe` across `*.rs` under `rmath-rs/` + `crates/`: **{report['unsafe_matching_lines']}**",
    f"- Method: `{report['unsafe_method']}`",
    "",
    "## Safety Gates (nightly, workflow-declared)",
    "",
    f"- Miri: {report['miri']}",
    f"- GC torture: {report['gc_torture']}",
    "",
]
(report_dir / "compatibility-report.md").write_text("\n".join(lines))

print((report_dir / "compatibility-report.md").read_text())
PY

if [[ "$status" -ne 0 && "$CHECK" -eq 1 ]]; then
    echo "compat_report: ${status} failing probe(s) (see PROBE-FAIL lines above)" >&2
    exit "$status"
fi

echo "Wrote $REPORT_DIR/compatibility-report.md"
echo "Wrote $REPORT_DIR/compatibility-report.json"
