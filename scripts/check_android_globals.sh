#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ALLOWLIST="$ROOT_DIR/docs/android-platform-global-allowlist.tsv"

if [[ ! -f "$ALLOWLIST" ]]; then
    echo "Missing Android global allowlist: $ALLOWLIST" >&2
    exit 1
fi

tmp_found="$(mktemp)"
tmp_allowed="$(mktemp)"
tmp_new="$(mktemp)"
tmp_stale="$(mktemp)"
cleanup() {
    rm -f "$tmp_found" "$tmp_allowed" "$tmp_new" "$tmp_stale"
}
trap cleanup EXIT

cd "$ROOT_DIR"

rg -l 'thread_local!|static mut|OnceLock|LazyLock|Mutex<|RwLock<|Atomic[A-Za-z]+|lazy_static' \
    rmath-rs/rmath/src --glob '*.rs' | sort > "$tmp_found"

awk -F '\t' 'NF && $1 !~ /^#/ { print $1 }' "$ALLOWLIST" | sort > "$tmp_allowed"

comm -23 "$tmp_found" "$tmp_allowed" > "$tmp_new"
comm -13 "$tmp_found" "$tmp_allowed" > "$tmp_stale"

if [[ -s "$tmp_new" || -s "$tmp_stale" ]]; then
    if [[ -s "$tmp_new" ]]; then
        echo "Unclassified mutable global files:" >&2
        sed 's/^/  /' "$tmp_new" >&2
    fi
    if [[ -s "$tmp_stale" ]]; then
        echo "Stale Android global allowlist entries:" >&2
        sed 's/^/  /' "$tmp_stale" >&2
    fi
    echo "Update docs/android-platform-global-allowlist.tsv or remove the global state." >&2
    exit 1
fi

echo "Android mutable-global allowlist is current."
