#!/usr/bin/env bash
# Print same-font GNU R metrics; R, a C compiler, pkg-config and FreeType required.
set -euo pipefail
repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/rport-plotmath-oracle.XXXXXX")"
trap 'rm -rf -- "$work_dir"' EXIT
r_bin="${R_BIN:-R}"
cp -f "$repo_dir/scripts/plotmath_font_oracle.c" "$work_dir/plotmath_font_oracle.c"
PKG_CPPFLAGS="$(pkg-config --cflags freetype2)" \
PKG_LIBS="$(pkg-config --libs freetype2)" \
    "$r_bin" CMD SHLIB -o "$work_dir/plotmath_font_oracle.so" "$work_dir/plotmath_font_oracle.c" >/dev/null
RPORT_DEJAVU_DIR="$repo_dir/crates/r-graphics-engine/assets" \
    "$r_bin" --vanilla --slave --file="$repo_dir/scripts/plotmath_font_oracle.R" \
    --args "$work_dir/plotmath_font_oracle.so"
