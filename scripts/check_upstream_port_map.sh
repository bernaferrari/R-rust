#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAP="$ROOT_DIR/docs/upstream-port-map.tsv"

if [[ ! -f "$MAP" ]]; then
    echo "Missing upstream port map: $MAP" >&2
    exit 1
fi

cd "$ROOT_DIR"

allowed_modes=$'\nfaithful\nrust-shaped\npolicy\ngenerated\nknown-gap\n'
failures=0
rows=0

while IFS=$'\t' read -r rust_path upstream_paths anchor mode notes extra; do
    [[ -z "${rust_path:-}" || "$rust_path" == \#* ]] && continue
    rows=$((rows + 1))

    if [[ -n "${extra:-}" ]]; then
        echo "$MAP:$rows: expected exactly 5 tab-separated columns" >&2
        failures=$((failures + 1))
        continue
    fi

    if [[ ! -e "$rust_path" ]]; then
        echo "$MAP:$rows: Rust path does not exist: $rust_path" >&2
        failures=$((failures + 1))
    fi

    if [[ "$allowed_modes" != *$'\n'"$mode"$'\n'* ]]; then
        echo "$MAP:$rows: unknown sync mode: $mode" >&2
        failures=$((failures + 1))
    fi

    if [[ -z "$anchor" || -z "$notes" ]]; then
        echo "$MAP:$rows: anchor and notes must be non-empty" >&2
        failures=$((failures + 1))
    fi

    if [[ "$upstream_paths" != "none" ]]; then
        IFS=',' read -ra paths <<< "$upstream_paths"
        for upstream_path in "${paths[@]}"; do
            upstream_path="${upstream_path#"${upstream_path%%[![:space:]]*}"}"
            upstream_path="${upstream_path%"${upstream_path##*[![:space:]]}"}"
            if [[ ! -e "$upstream_path" ]]; then
                echo "$MAP:$rows: upstream path does not exist: $upstream_path" >&2
                failures=$((failures + 1))
            fi
        done
    fi
done < "$MAP"

if [[ "$rows" -eq 0 ]]; then
    echo "$MAP: no mapped rows found" >&2
    exit 1
fi

if [[ "$failures" -ne 0 ]]; then
    echo "Upstream port map check failed with $failures issue(s)." >&2
    exit 1
fi

echo "Upstream port map check passed: $rows mapped source anchors."
