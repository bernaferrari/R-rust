#!/usr/bin/env bash
# Run the pinned real-package corpus: unpack each vendored tarball into a
# fresh bundled-library dir, load it in the engine, run its probes, and
# compare against the pinned oracle where available.
#
# Usage: scripts/real_package_corpus.sh [--check] [--report DIR]
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_DIR="$ROOT_DIR/target/real-package-corpus"
CHECK=0

usage() {
    cat <<'USAGE'
Usage: scripts/real_package_corpus.sh [OPTIONS]

Unpacks tests/real-packages/vendor/*.tar.gz into a scratch library, loads
each package through r-embed, runs the manifest probes, and writes a
machine-readable report.

Options:
  --check          Fail when any package marked `pass` in the manifest fails.
  --report DIR     Write summary artifacts to DIR.
  -h, --help       Show this help.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --check) CHECK=1; shift ;;
        --report)
            if (($# < 2)); then usage >&2; exit 2; fi
            REPORT_DIR="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

cd "$ROOT_DIR"
mkdir -p "$REPORT_DIR"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/rport-real-pkgs.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

BUNDLED="$SCRATCH/bundled"
mkdir -p "$BUNDLED"
for tarball in tests/real-packages/vendor/*.tar.gz; do
    tar xzf "$tarball" -C "$BUNDLED"
done

APP="$SCRATCH/app"
CACHE="$SCRATCH/cache"
mkdir -p "$APP" "$CACHE"

LOG="$REPORT_DIR/run.log"
status=0
if env RPORT_REAL_PKG_BUNDLED="$BUNDLED" RPORT_REAL_PKG_APP="$APP" RPORT_REAL_PKG_CACHE="$CACHE" \
    cargo test -p r-embed real_package_corpus -- --nocapture >"$LOG" 2>&1; then
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

report = {
    "generated_at_utc": dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat(),
    "status": "pass" if status == 0 else "fail",
    "cargo_status": status,
    "manifest": "tests/real-packages/manifest.toml",
    "vendor": sorted(p.name for p in pathlib.Path("tests/real-packages/vendor").glob("*.tar.gz")),
    "log": str(log_path),
}

(report_dir / "summary.json").write_text(json.dumps(report, indent=2) + "\n")

lines = [
    "# Real-Package Corpus Report",
    "",
    f"Generated: `{report['generated_at_utc']}`",
    "",
    f"Status: **{report['status']}**",
    "",
    "## Vendored Sources",
    "",
]
lines.extend(f"- {name}" for name in report["vendor"])
lines.extend(["", "## Log", "", f"`{log_path}`", ""])
(report_dir / "summary.md").write_text("\n".join(lines))
print((report_dir / "summary.md").read_text())
PY

if [[ "$status" -ne 0 && "$CHECK" -eq 1 ]]; then
    sed 's/^/  cargo | /' "$LOG" >&2
    exit "$status"
fi

echo "Wrote $REPORT_DIR/summary.md"
echo "Wrote $REPORT_DIR/summary.json"
