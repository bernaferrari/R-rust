#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_DIR="$ROOT_DIR/target/release-gate/conformance"
FULL=0
RUN_ANDROID=1
RUN_ANDROID_PACKAGE=0
RUN_UNIFFI_BINDINGS=0
RUN_DESKTOP_SMOKE=0
RUN_STRICT_CLIPPY=0

usage() {
    cat <<'USAGE'
Usage: scripts/release_gate.sh [OPTIONS]

Runs the local release-candidate gate:
  - rustfmt check
  - targeted Rust tests for rmath, r-embed, and r-uniffi
  - Android mutable-global scan and aarch64 cargo check
  - C R vs Rust conformance report
  - generated artifact sanity checks
  - git whitespace check

Options:
  --full              Also run desktop host smoke, UniFFI binding generation,
                      and Android Gradle packaging smoke.
  --no-android        Skip Android SDK/NDK checks. This is for host-only
                      development and is not a release gate.
  --android-package   Run scripts/android_package_smoke.sh --check.
  --uniffi-bindings   Run scripts/generate_uniffi_bindings.sh --check.
  --desktop-smoke     Run scripts/desktop_host_smoke.sh.
  --strict-clippy     Run the current aspirational clippy gate. This is tracked
                      by bead rport-0dbg and is not expected to pass yet.
  -h, --help          Show this help.
USAGE
}

while (($# > 0)); do
    case "$1" in
        --full)
            FULL=1
            RUN_ANDROID_PACKAGE=1
            RUN_UNIFFI_BINDINGS=1
            RUN_DESKTOP_SMOKE=1
            shift
            ;;
        --no-android)
            RUN_ANDROID=0
            shift
            ;;
        --android-package)
            RUN_ANDROID_PACKAGE=1
            shift
            ;;
        --uniffi-bindings)
            RUN_UNIFFI_BINDINGS=1
            shift
            ;;
        --desktop-smoke)
            RUN_DESKTOP_SMOKE=1
            shift
            ;;
        --strict-clippy)
            RUN_STRICT_CLIPPY=1
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

cd "$ROOT_DIR"

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi

section() {
    printf '\n==> %s\n' "$1"
}

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    "$@"
}

run_cargo() {
    run env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" cargo "$@"
}

check_conformance_artifacts() {
    local summary_json="$REPORT_DIR/summary.json"
    local summary_md="$REPORT_DIR/summary.md"

    [[ -s "$summary_json" ]] || {
        echo "Missing conformance JSON artifact: $summary_json" >&2
        exit 1
    }
    [[ -s "$summary_md" ]] || {
        echo "Missing conformance Markdown artifact: $summary_md" >&2
        exit 1
    }

    run python3 - "$summary_json" <<'PY'
import json
import pathlib
import sys

summary_path = pathlib.Path(sys.argv[1])
summary = json.loads(summary_path.read_text())
failed = int(summary.get("failed", 0))
unexpected = int(summary.get("unexpected_passes", 0))
total = int(summary.get("total", 0))
passed = int(summary.get("passed", 0))

if total <= 0:
    raise SystemExit(f"{summary_path}: conformance report has no cases")
if failed or unexpected:
    raise SystemExit(
        f"{summary_path}: failed={failed}, unexpected_passes={unexpected}"
    )

print(f"Conformance artifact OK: {passed}/{total} cases passing.")
PY
}

if [[ "$FULL" -eq 1 ]]; then
    echo "Release gate mode: full"
else
    echo "Release gate mode: default"
fi
echo "Rust warning policy: using RUSTFLAGS='$RUSTFLAGS_FOR_BUILD'."
echo "Strict clippy zero-warning cleanup is tracked by bead rport-0dbg."

section "Rust formatting"
run cargo fmt --check --all

if [[ "$RUN_STRICT_CLIPPY" -eq 1 ]]; then
    section "Strict clippy"
    run cargo clippy --all-targets --all-features -- -D warnings
fi

section "Rust tests"
run_cargo test -p rmath -- --test-threads=1
run_cargo test -p r-embed -p r-uniffi -- --test-threads=1

if [[ "$RUN_ANDROID" -eq 1 ]]; then
    section "Android toolchain and mutable globals"
    run scripts/android_toolchain_check.sh
else
    section "Android toolchain and mutable globals"
    echo "Skipped by --no-android. Do not use this mode for release signoff."
fi

section "Conformance report"
run scripts/conformance_parity.sh --check --report "$REPORT_DIR"

section "Artifact sanity"
check_conformance_artifacts

section "Public safe API audit"
run scripts/audit_safe_api.sh

if [[ "$RUN_DESKTOP_SMOKE" -eq 1 ]]; then
    section "Desktop host smoke"
    run scripts/desktop_host_smoke.sh
fi

if [[ "$RUN_UNIFFI_BINDINGS" -eq 1 ]]; then
    section "UniFFI binding generation"
    run scripts/generate_uniffi_bindings.sh --check
fi

if [[ "$RUN_ANDROID_PACKAGE" -eq 1 ]]; then
    section "Android package smoke"
    run scripts/android_package_smoke.sh --check
fi

section "Git whitespace"
run git diff --check

echo
echo "Release gate passed. Conformance report: $REPORT_DIR/summary.md"
