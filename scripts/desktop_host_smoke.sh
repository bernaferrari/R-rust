#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATE="${HOST_SMOKE_CRATE:-r-host-cli}"
INPUT="${HOST_SMOKE_INPUT:-q()}"

tmp_output="$(mktemp)"
trap 'rm -f "$tmp_output"' EXIT

cd "$ROOT_DIR"
printf '%s\n' "$INPUT" | cargo run -p "$CRATE" --quiet | tr -d '\r' | tee "$tmp_output"

grep -q "rport R interpreter (desktop host)" "$tmp_output"
grep -q "Type R expressions" "$tmp_output"

echo "Desktop host smoke passed for ${CRATE}."
