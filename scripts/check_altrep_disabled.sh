#!/usr/bin/env bash
# Assert the ALTREP feature stays OFF in default / release-shaped builds.
#
# ALTREP currently stores ordinary Rust heap pointers in SEXP payload slots
# that the generational GC traces as SEXP references — enabling the feature
# corrupts the heap under collection. Dependents must not flip it on.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CARGO_TOML="crates/rmath/Cargo.toml"
if [[ ! -f "$CARGO_TOML" ]]; then
  echo "error: missing $CARGO_TOML" >&2
  exit 1
fi

python3 - "$CARGO_TOML" <<'PY'
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - py<3.11
    import tomli as tomllib  # type: ignore

path = Path(sys.argv[1])
data = tomllib.loads(path.read_text())
features = data.get("features") or {}
default = features.get("default") or []
if "altrep" in default:
    raise SystemExit(
        f"{path}: feature 'altrep' must NOT be in default={default!r}"
    )
if features.get("altrep") != []:
    raise SystemExit(f"{path}: expected an explicit empty 'altrep = []' feature gate")
print(f"OK: {path} keeps altrep out of default features ({default})")
PY

# Resolve workspace feature unification too: a default alias or dependent
# enabling rmath/altrep must fail even when rmath's literal defaults look safe.
cargo metadata --format-version 1 --locked | python3 -c '
import json, sys
metadata = json.load(sys.stdin)
rmath_ids = {p["id"] for p in metadata["packages"] if p["name"] == "rmath"}
for node in metadata["resolve"]["nodes"]:
    if node["id"] in rmath_ids and "altrep" in node["features"]:
        raise SystemExit("error: workspace dependency resolution enables rmath/altrep")
print("OK: resolved workspace features keep ALTREP disabled")
'

# Default `scripts/cargo_dev.sh test -p rmath` must compile and run without --features altrep.
# The sexp::no_altrep_guards tests pin cfg!(feature = "altrep") == false.
echo "Running default-build altrep-off unit guards..."
scripts/cargo_dev.sh test -p rmath --lib sexp::no_altrep_guards -- --test-threads=1

echo "check_altrep_disabled: PASS"
