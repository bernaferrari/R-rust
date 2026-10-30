#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
scripts/cargo_dev.sh run -q -p rport-android-global-audit -- \
    --check "$ROOT_DIR" "$ROOT_DIR/docs/android-platform-global-declarations.tsv"
echo "Android mutable-global declaration inventory is current."
echo "Inventory classification alone does not establish session isolation."
