#!/usr/bin/env bash
# The pure world propagates R errors, return/break and restarts through
# Result values; nothing unwinds, so the runtime builds with the wasm
# default panic=abort against the stock precompiled std.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
WASM_TOOLCHAIN="nightly-2026-08-25"
export RUSTUP_TOOLCHAIN="$WASM_TOOLCHAIN"
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    # Cargo can reuse a path dependency from a different worktree when both
    # builds use the same target directory. Keep each checkout's fingerprints
    # under the caller's chosen cache root. The output package path is unchanged.
    checkout_key="$(python3 -c 'import hashlib, sys; print(hashlib.sha256(sys.argv[1].encode()).hexdigest()[:16])' "$ROOT_DIR")"
    export CARGO_TARGET_DIR="$CARGO_TARGET_DIR/rport-checkouts/$checkout_key"
else
    export CARGO_TARGET_DIR="$ROOT_DIR/target/wasm-unwind"
fi
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Clink-arg=--max-memory=268435456"
if ! rustup run "$WASM_TOOLCHAIN" rustc --version >/dev/null 2>&1; then
    echo "Install prerequisites: rustup toolchain install $WASM_TOOLCHAIN --profile minimal --target wasm32-unknown-unknown" >&2
    exit 2
fi
WASM_PACK_ARGS=("$@")
OUTPUT_DIR="pkg"
OPTIMIZE=1
while (($# > 0)); do
    case "$1" in
        --out-dir) OUTPUT_DIR="$2"; shift 2 ;;
        --out-dir=*) OUTPUT_DIR="${1#--out-dir=}"; shift ;;
        --dev|--profiling) OPTIMIZE=0; shift ;;
        *) shift ;;
    esac
done
if [[ "$OPTIMIZE" == 1 ]]; then
    # Level 2 keeps the distributed Wasm within the size and browser time
    # budgets while preserving the caller's explicit profile override.
    export CARGO_PROFILE_RELEASE_OPT_LEVEL="${CARGO_PROFILE_RELEASE_OPT_LEVEL:-2}"
fi

# Skip wasm-pack's old bundled optimizer, which crashes on Rust's Wasm EH.
# Release assets use the separately pinned, verified Binaryen below.
# Optional GPU builds stay separate from the default CPU distribution.
if [[ -n "${RPORT_WASM_FEATURES:-}" ]]; then
    wasm-pack build crates/r-wasm --no-opt "${WASM_PACK_ARGS[@]}" -- --features "$RPORT_WASM_FEATURES"
else
    wasm-pack build crates/r-wasm --no-opt "${WASM_PACK_ARGS[@]}"
fi

if [[ "$OPTIMIZE" == 1 ]]; then
    if [[ "$OUTPUT_DIR" != /* ]]; then OUTPUT_DIR="$ROOT_DIR/crates/r-wasm/$OUTPUT_DIR"; fi
    python3 "$ROOT_DIR/scripts/optimize_wasm_runtime.py" --package "$OUTPUT_DIR"
fi
