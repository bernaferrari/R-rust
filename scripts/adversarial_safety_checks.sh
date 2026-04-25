#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ITERATIONS=256

while [[ $# -gt 0 ]]; do
    case "$1" in
        --long)
            ITERATIONS=4096
            shift
            ;;
        --iterations)
            ITERATIONS="$2"
            shift 2
            ;;
        --check)
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

cd "$ROOT_DIR"
export RPORT_ADVERSARIAL_ITERS="$ITERATIONS"

cargo test -p rmath adversarial -- --test-threads=1
