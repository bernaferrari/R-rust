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

line_is_in_cfg_test_module() {
    local file="$1"
    local target_line="$2"
    awk -v target="$target_line" '
        function count_lbraces(s, tmp) {
            tmp = s
            return gsub(/\{/, "", tmp)
        }
        function count_rbraces(s, tmp) {
            tmp = s
            return gsub(/\}/, "", tmp)
        }
        NR > target { exit }
        {
            if (in_test) {
                depth += count_lbraces($0) - count_rbraces($0)
                if (NR == target) {
                    found = 1
                    exit
                }
                if (depth <= 0) {
                    in_test = 0
                }
            }

            if ($0 ~ /^[[:space:]]*#\[cfg\(test\)\]/) {
                pending_cfg_test = 1
                next
            }

            if (pending_cfg_test && $0 ~ /^[[:space:]]*mod[[:space:]]+tests[[:space:]]*\{/) {
                in_test = 1
                depth = count_lbraces($0) - count_rbraces($0)
                pending_cfg_test = 0
                if (NR == target) {
                    found = 1
                    exit
                }
                if (depth <= 0) {
                    in_test = 0
                }
                next
            }

            if (pending_cfg_test && $0 !~ /^[[:space:]]*$/ && $0 !~ /^[[:space:]]*#/) {
                pending_cfg_test = 0
            }
        }
        END { exit found ? 0 : 1 }
    ' "$file"
}

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
} | while IFS=$'\t' read -r file kind line text; do
    [[ -n "$file" ]] || continue
    if line_is_in_cfg_test_module "$file" "$line"; then
        continue
    fi
    printf '%s\t%s\t%s\t%s\n' "$file" "$kind" "$line" "$text"
done | awk -F '\t' '$4 !~ /OnceLock<usize>/ { print }' | sort -u > "$tmp_details"

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
echo "Scanned declaration kinds: static mut, thread_local!, lazy_static!/static ref, and static OnceLock/LazyLock/Mutex/RwLock/Atomic* definitions (excluding #[cfg(test)] modules and OnceLock<usize> immutable sentinels)."
