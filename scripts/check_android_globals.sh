#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ALLOWLIST="$ROOT_DIR/docs/android-platform-global-allowlist.tsv"

if [[ ! -f "$ALLOWLIST" ]]; then
    echo "Missing Android global allowlist: $ALLOWLIST" >&2
    exit 1
fi

tmp_found="$(mktemp)"
tmp_details="$(mktemp)"
tmp_allowed="$(mktemp)"
tmp_new="$(mktemp)"
tmp_stale="$(mktemp)"
cleanup() {
    rm -f "$tmp_found" "$tmp_details" "$tmp_allowed" "$tmp_new" "$tmp_stale"
}
trap cleanup EXIT

cd "$ROOT_DIR"

scan_kind() {
    local kind="$1"
    local pattern="$2"
    local engine="${3:-}"

    {
        if [[ -n "$engine" ]]; then
            rg "$engine" -n --no-heading --glob '*.rs' "$pattern" rmath-rs/rmath/src || true
        else
            rg -n --no-heading --glob '*.rs' "$pattern" rmath-rs/rmath/src || true
        fi
    } | awk -F: -v kind="$kind" '
        {
            file = $1
            line = $2
            text = substr($0, length(file) + length(line) + 3)
            sub(/^[[:space:]]+/, "", text)
            print file "\t" kind "\t" line "\t" text
        }
    '
}

{
    scan_kind "static-mut" '^\s*static\s+mut\b'
    scan_kind "thread-local" '^\s*thread_local!\s*\{'
    scan_kind "lazy-static" '^\s*lazy_static!\s*\{'
    scan_kind "lazy-static-ref" '^\s*static\s+ref\b'
    scan_kind "sync-static" \
        '^\s*static\s+(?!ref\b)[A-Za-z_][A-Za-z0-9_]*\s*:\s*.*\b(?:OnceLock|LazyLock|Mutex|RwLock|Atomic[A-Za-z]+)\b' \
        '-P'
} | awk -F '\t' '$4 !~ /\bOnceLock<usize>\b/ { print }' | sort -u > "$tmp_details"

awk -F '\t' 'NF { print $1 }' "$tmp_details" | sort -u > "$tmp_found"

awk -F '\t' 'NF && $1 !~ /^#/ { print $1 }' "$ALLOWLIST" | sort > "$tmp_allowed"

comm -23 "$tmp_found" "$tmp_allowed" > "$tmp_new"
comm -13 "$tmp_found" "$tmp_allowed" > "$tmp_stale"

if [[ -s "$tmp_new" || -s "$tmp_stale" ]]; then
    if [[ -s "$tmp_new" ]]; then
        echo "Unclassified mutable global files:" >&2
        while IFS= read -r file; do
            echo "  $file" >&2
            awk -F '\t' -v file="$file" '
                $1 == file {
                    printf "    - %s:%s: %s\n", $2, $3, $4
                }
            ' "$tmp_details" >&2
        done < "$tmp_new"
    fi
    if [[ -s "$tmp_stale" ]]; then
        echo "Stale Android global allowlist entries:" >&2
        sed 's/^/  /' "$tmp_stale" >&2
    fi
    echo "Update docs/android-platform-global-allowlist.tsv or remove the global state." >&2
    exit 1
fi

echo "Android mutable-global allowlist is current."
echo "Scanned declaration kinds: static mut, thread_local!, lazy_static!/static ref, and static OnceLock/LazyLock/Mutex/RwLock/Atomic* definitions (excluding OnceLock<usize> immutable sentinels)."
