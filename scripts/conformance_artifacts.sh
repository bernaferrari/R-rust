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

# Local callers retain the historical debug profile. CI explicitly opts into
# the existing release profile for repeated full-runtime startup; Cargo config
# and the user's compilation settings remain untouched.
conformance_profile() {
    local profile="${RPORT_CONFORMANCE_PROFILE:-debug}"
    case "$profile" in
        debug|release) printf '%s' "$profile" ;;
        *) printf 'Invalid conformance profile: %s\n' "$profile" >&2; return 2 ;;
    esac
}

conformance_cargo() {
    local action="$1"
    shift
    local profile
    profile="$(conformance_profile)" || return
    local options=()
    if [[ "$profile" == release ]]; then options+=(--release); fi
    (cd "$ROOT_DIR" && "$ROOT_DIR/scripts/cargo_dev.sh" "$action" ${options[@]+"${options[@]}"} "$@")
}

conformance_dependency_dir() {
    local profile
    profile="$(conformance_profile)" || return
    printf '%s/%s/deps' "$(conformance_target_dir)" "$profile"
}

conformance_find_rmath_rlib() {
    local target_dir="${1:-$(conformance_target_dir)}"
    local profile
    profile="$(conformance_profile)" || return
    local found=""
    local candidate
    local candidates=()

    shopt -s nullglob
    candidates+=(
        "$target_dir/$profile/deps/librmath-"*.rlib
        "$target_dir/$profile/deps/librmath.rlib"
        "$target_dir/$profile/librmath.rlib"
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

# Admit only the exact library emitted by this build, even when Cargo reused it.
# A scoped subshell owns cleanup without replacing a caller's EXIT trap.
conformance_build_rmath() (
    local receipt status
    receipt="$(mktemp "${TMPDIR:-/tmp}/rport-conformance-cargo.XXXXXX")" || exit
    trap 'rm -f "$receipt"' EXIT
    if conformance_cargo build -p rmath "$@" --message-format=json >"$receipt"; then
        python3 "$ROOT_DIR/scripts/conformance_cargo_artifact.py" "$receipt"
    else
        status=$?
        cat "$receipt" >&2
        exit "$status"
    fi
)
