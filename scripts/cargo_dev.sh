#!/usr/bin/env bash
# Run Cargo unchanged, then bound obsolete local executable variants.
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PRUNE_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
previous=""
for argument in "$@"; do
    if [[ "$previous" == "--target-dir" ]]; then
        PRUNE_TARGET_DIR="$argument"
    elif [[ "$argument" == --target-dir=* ]]; then
        PRUNE_TARGET_DIR="${argument#--target-dir=}"
    fi
    previous="$argument"
done
# Bound native math/thread pools so cargo runs stay within ~2-3 cores
# even when Accelerate/vecLib ignores Cargo's jobserver.
export VECLIB_MAXIMUM_THREADS="${VECLIB_MAXIMUM_THREADS:-1}"
export OMP_NUM_THREADS="${OMP_NUM_THREADS:-1}"
export RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-2}"

cargo "$@"
result=$?
case "${1:-}" in
    build|check|test|clippy)
        prune_options=(--target-dir "$PRUNE_TARGET_DIR" --apply)
        if [[ -n "${RPORT_CARGO_ARTIFACTS_JSON:-}" ]]; then
            prune_options+=(--artifacts-json "$RPORT_CARGO_ARTIFACTS_JSON")
        fi
        python3 "$ROOT_DIR/scripts/prune_build_binaries.py" \
            "${prune_options[@]}" || \
            echo "Build-cache pruning skipped; Cargo's exit status is unchanged." >&2
        ;;
esac
exit "$result"
