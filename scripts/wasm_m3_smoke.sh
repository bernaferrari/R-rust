#!/usr/bin/env bash
# M3 smoke test (docs/web-architecture.md): build the r-wasm boundary with
# wasm-pack and run the real oracle assertions under Node.
#
#   eval("1+1") === "[1] 2"
#   is_input_complete("1 + 1") === true
#   is_input_complete("f <- function(x) {") === false
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE_DIR="$ROOT_DIR/crates/r-wasm"

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "wasm-pack is required for the M3 smoke test (cargo install wasm-pack)" >&2
    exit 2
fi
if ! command -v node >/dev/null 2>&1; then
    echo "node is required for the M3 smoke test" >&2
    exit 2
fi

rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true

cd "$ROOT_DIR"
echo "Building r-wasm with wasm-pack (nodejs target)..."
wasm-pack build "$CRATE_DIR" --target nodejs --dev

echo "Running node smoke test..."
cd "$CRATE_DIR"
exec node smoke.mjs
