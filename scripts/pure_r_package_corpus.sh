#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_DIR="$ROOT_DIR/target/pure-r-package-corpus"
CHECK=0

usage() {
    cat <<'USAGE'
Usage: scripts/pure_r_package_corpus.sh [OPTIONS]

Runs the release-facing pure-R package compatibility corpus. The corpus covers
Android-style library paths, package metadata discovery, DESCRIPTION metadata
readers, namespace-only loading, namespace-qualified access,
Depends, imports/importFrom/exportPattern, source-form package data, explicit
data environments, package resource/example lookup through system.file(),
DESCRIPTION Collate source ordering, source-form LazyData, serialized data
policy errors, same-name package isolation across sessions, S4 package code,
package-visible library paths, and explicit rejection of native/compiled/
bytecode packages.

Options:
  --check          Fail when any corpus test fails.
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

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi

LOG="$REPORT_DIR/cargo-test.log"
status=0
if env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" cargo test -p r-embed pure_r_package_corpus -- --nocapture >"$LOG" 2>&1; then
    status=0
else
    status=$?
fi

python3 - "$REPORT_DIR" "$LOG" "$status" <<'PY'
import datetime as dt
import json
import pathlib
import sys

report_dir = pathlib.Path(sys.argv[1])
log_path = pathlib.Path(sys.argv[2])
status = int(sys.argv[3])

scenarios = [
    "installed package metadata and library path discovery",
    "DESCRIPTION metadata readers through packageVersion() and packageDescription()",
    "namespace-only loading through requireNamespace(), getNamespace(), and loadedNamespaces()",
    "namespace-qualified access through pkg::name and pkg:::name",
    "library() loads pure-R namespaces",
    "DESCRIPTION Depends package loading and dependency export visibility",
    "export, S3 method, import, importFrom, and exportPattern directives",
    "source-form package data listing and loading",
    "source-form package data loading into an explicit environment",
    "package resource and example file lookup through system.file()",
    "DESCRIPTION Collate source ordering for source-form package code",
    "source-form LazyData exposure through library()",
    "S4 class creation and slot access from package code",
    "package-visible Android library paths",
    "serialized lazy-data policy rejection",
    "explicit native-code, compiled-code, and bytecode package rejection",
    "same-name package isolation across sessions",
]

report = {
    "generated_at_utc": dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat(),
    "status": "pass" if status == 0 else "fail",
    "cargo_status": status,
    "scenarios": scenarios,
    "log": str(log_path),
}

(report_dir / "summary.json").write_text(json.dumps(report, indent=2) + "\n")

lines = [
    "# Pure-R Package Corpus Report",
    "",
    f"Generated: `{report['generated_at_utc']}`",
    "",
    f"Status: **{report['status']}**",
    "",
    "## Covered Scenarios",
    "",
]
lines.extend(f"- {scenario}" for scenario in scenarios)
lines.extend([
    "",
    "## Log",
    "",
    f"`{log_path}`",
    "",
])
(report_dir / "summary.md").write_text("\n".join(lines))

print((report_dir / "summary.md").read_text())
PY

if [[ "$status" -ne 0 && "$CHECK" -eq 1 ]]; then
    sed 's/^/  cargo | /' "$LOG" >&2
    exit "$status"
fi

echo "Wrote $REPORT_DIR/summary.md"
echo "Wrote $REPORT_DIR/summary.json"
