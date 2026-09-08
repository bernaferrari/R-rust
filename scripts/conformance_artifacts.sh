#!/usr/bin/env bash

# Shared artifact-path helpers for the conformance runners.  Cargo resolves a
# relative CARGO_TARGET_DIR from the directory where it is invoked; all
# runners invoke Cargo from ROOT_DIR, so resolve relative values against that
# directory as well.  The default is a conformance-only target so these
# runners do not race ordinary workspace builds in target/debug.  Keeping this
# in one place prevents a configured build from silently falling back to a
# stale repository-level target.

if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
    export CARGO_TARGET_DIR="$ROOT_DIR/target/conformance"
fi

conformance_target_dir() {
    local configured="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
    if [[ "$configured" == /* ]]; then
        printf '%s' "$configured"
    else
        printf '%s/%s' "$ROOT_DIR" "$configured"
    fi
}

conformance_dependency_dir() {
    printf '%s/debug/deps' "$(conformance_target_dir)"
}

conformance_find_rmath_rlib() {
    local target_dir="${1:-$(conformance_target_dir)}"
    local found=""
    local candidate
    local candidates=()

    shopt -s nullglob
    candidates+=(
        "$target_dir/debug/deps/librmath-"*.rlib
        "$target_dir/debug/deps/librmath.rlib"
        "$target_dir/debug/librmath.rlib"
    )
    shopt -u nullglob

    for candidate in "${candidates[@]}"; do
        [[ -f "$candidate" ]] || continue
        if [[ -z "$found" || "$candidate" -nt "$found" ]]; then
            found="$candidate"
        fi
    done
    printf '%s' "$found"
}
