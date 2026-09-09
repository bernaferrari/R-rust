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

cargo "$@"
result=$?
case "${1:-}" in
    build|check|test|clippy)
        python3 "$ROOT_DIR/scripts/prune_build_binaries.py" \
            --target-dir "$PRUNE_TARGET_DIR" --apply || \
            echo "Build-cache pruning skipped; Cargo's exit status is unchanged." >&2
        ;;
esac
exit "$result"
