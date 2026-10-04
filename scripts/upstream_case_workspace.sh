#!/usr/bin/env bash
# Each engine receives its own complete copy of the case's sibling fixtures.
# SRCDIR refers to that copy so relative and SRCDIR-based writes cannot change
# the imported corpus or become inputs to the other engine.
upstream_stage_case_workspace() {
    local source_dir="$1"
    local workspace="$2"
    mkdir -p "$workspace" && cp -rf "$source_dir/." "$workspace/"
}

upstream_run_case_workspace() {
    local workspace="$1"
    local case_basename="$2"
    shift 2
    (
        cd "$workspace" &&
            env LC_ALL=C LANG=C TZ=UTC SRCDIR="$workspace" \
                "$@" "$case_basename"
    )
}
