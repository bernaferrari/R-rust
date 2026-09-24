#!/usr/bin/env bash
# Kani 0.68.0, compiler nightly-2026-08-21.
# Does not replace Miri, GC torture, fuzz, or conformance_parity.sh.
# Unwinding assertions stay enabled. Do not pass --unwinding-assertions off.
#
# The workspace rust-toolchain.toml stays on Rust 1.96.0. This script runs
# from kani/, whose toolchain file selects Kani's nightly, then points cargo
# at the rmath manifest.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/kani"
active="$(rustup show active-toolchain)"
printf '%s\n' "$active" | grep -q 'nightly-2026-08-21' || {
    echo "cargo_kani: expected nightly-2026-08-21, got ${active}" >&2
    exit 1
}
# rmath does not compile with no linear-algebra backend. Keep graphics off
# (--no-default-features) and enable only rust-backend, which pulls faer.
echo "cargo kani --manifest-path ${ROOT}/crates/rmath/Cargo.toml --no-default-features --features rust-backend $*"
exec cargo kani --manifest-path "${ROOT}/crates/rmath/Cargo.toml" --no-default-features --features rust-backend "$@"
