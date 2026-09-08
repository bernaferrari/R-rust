#!/usr/bin/env bash
# libc ratchet: the engine (crates/rmath/src) is on a one-way street to
# zero libc. Each category has a committed budget in scripts/libc-budget.txt;
# this gate FAILS if any live count exceeds its budget. Shrinking a count?
# Also shrink the budget in the same commit — budgets only go down.
#
# Usage: scripts/libc_ratchet.sh [--update-hint]
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT_DIR/crates/rmath/src"
BUDGET="$ROOT_DIR/scripts/libc-budget.txt"

count() {
  # Comment-only lines do not consume libc: budgets count code. grep exits
  # 1 on zero matches; the || true keeps set -e from aborting the gate.
  { grep -rE "$1" "$SRC" --include='*.rs' 2>/dev/null || true; } | grep -vE "^[^:]+:[0-9]+:[[:space:]]*(//|/\*|\*)" | wc -l | tr -d ' ' || true
}

# category<TAB>regex — live counts computed fresh each run.
declare -a CATS=(
  "variadic_printf:libc::(snprintf|sprintf|fprintf|vsnprintf|vsprintf|vfprintf)|rport_snprintf"
  "heap:libc::(malloc|calloc|realloc|free)\b"
  "env:libc::(getenv|setenv|unsetenv|putenv|environ)\b"
  "time_fns:libc::(mktime|localtime|gmtime|strftime|tzset|time)\b"
  "c_type_aliases:libc::c_(int|char|void|long|double|uint|ulong|short|uchar|float)"
  "string_mem:libc::(strlen|strcmp|strncmp|strcpy|strncpy|strcat|strncat|strchr|strrchr|strstr|strdup|strndup|memcmp|memcpy|memmove|memset)\b"
  "stdio_FILE:libc::(FILE|fopen|fclose|fread|fwrite|fgets|fputs|fputc|fgetc|fflush|fseek|ftell|rewind|feof|ferror|remove|rename|tmpfile)\b"
)

fail=0
echo "libc ratchet (budgets live in scripts/libc-budget.txt):"
for entry in "${CATS[@]}"; do
  cat="${entry%%:*}"; re="${entry#*:}"
  live=$(count "$re")
  budget=$(awk -F'\t' -v c="$cat" '$1==c {print $2}' "$BUDGET" 2>/dev/null || echo "")
  if [[ -z "$budget" ]]; then
    echo "  BUDGET-MISSING  $cat=$live (add '$cat<TAB>$live' to scripts/libc-budget.txt)"
    fail=1
    continue
  fi
  if (( live > budget )); then
    echo "  OVER-BUDGET     $cat=$live > budget=$budget — replace the new libc use with Rust"
    fail=1
  elif (( live < budget )); then
    echo "  under (shrink?) $cat=$live < budget=$budget — lower the budget in this commit"
  else
    echo "  ok              $cat=$live"
  fi
done

exit $fail
