#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

APP_CRATES=(crates/r-embed/src crates/r-uniffi/src)

raw_public_pattern='^\s*pub(\([^)]*\))?\s+(use\s+.*\b(SEXP|SEXPTYPE|Sexprec|R_xlen_t|c_char|c_int|c_double)\b|(?:unsafe\s+)?(fn|struct|enum|type|trait|const|static)\b.*\b(SEXP|SEXPTYPE|Sexprec|R_xlen_t|c_char|c_int|c_double)\b)'
unsafe_pattern='^\s*(pub(\([^)]*\))?\s+)?unsafe\s+fn\b|unsafe\s*\{'

raw_hits="$(rg -n -P "$raw_public_pattern" "${APP_CRATES[@]}" || true)"
unsafe_hits="$(rg -n -P "$unsafe_pattern" "${APP_CRATES[@]}" || true)"

if [[ -n "$raw_hits" ]]; then
    echo "App-facing crates expose raw R/C runtime types:" >&2
    printf '%s\n' "$raw_hits" >&2
    exit 1
fi

if [[ -n "$unsafe_hits" ]]; then
    echo "App-facing crates contain unsafe code:" >&2
    printf '%s\n' "$unsafe_hits" >&2
    exit 1
fi

echo "Safe API audit passed."
echo "Checked crates: ${APP_CRATES[*]}"
echo "Policy: no public raw SEXP/SEXPTYPE/Sexprec/C scalar surface and no unsafe blocks in app-facing crates."
