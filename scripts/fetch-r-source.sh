#!/usr/bin/env bash
# Fetch the vendored upstream R source used as the diff-and-verify reference.
#
# The reference tree is NOT part of the crate build and is NOT committed to
# this repository (see .gitignore). Reproduce it exactly with this script:
#
#     ./scripts/fetch-r-source.sh [destination]
#
# The checkout is pinned to an exact commit so every machine diffs against
# the same upstream tree. The pin is the base of the last documented upstream
# sync (plans/upstream-sync-2026-08/); the parity oracle (a locally built
# trunk R) sits exactly 273 commits later in the same history:
#
#   vendored pin : d4cc5d9e196a144bbb087a798bb945b37121383b
#   trunk oracle : bac583951b728e97b9786804d3b4081f0fe18df5  (r79999)
#
# A full (non-shallow) clone is used so the upstream sync workflow
# (`git -C r-source fetch origin trunk` + history deltas) keeps working.
set -euo pipefail

PINNED_COMMIT="d4cc5d9e196a144bbb087a798bb945b37121383b"
REMOTE="https://github.com/wch/r-source.git"
DEST="${1:-r-source}"

fail() { echo "ERROR: $*" >&2; exit 1; }

if [ -e "$DEST" ] && [ ! -d "$DEST/.git" ]; then
  fail "$DEST exists but is not a git checkout; remove it and retry"
fi

if [ -d "$DEST/.git" ]; then
  current="$(git -C "$DEST" rev-parse HEAD)"
  if [ "$current" = "$PINNED_COMMIT" ]; then
    echo "r-source already at pinned commit $PINNED_COMMIT"
  else
    echo "r-source is at $current; fetching pinned commit $PINNED_COMMIT" >&2
    if ! git -C "$DEST" checkout --quiet "$PINNED_COMMIT" 2>/dev/null; then
      git -C "$DEST" fetch origin "$PINNED_COMMIT" ||
        git -C "$DEST" fetch origin
      git -C "$DEST" checkout --quiet "$PINNED_COMMIT" ||
        fail "could not check out pinned commit $PINNED_COMMIT"
    fi
  fi
else
  echo "Cloning $REMOTE into $DEST (full history, ~hundreds of MB)..." >&2
  git clone "$REMOTE" "$DEST"
  git -C "$DEST" checkout --quiet "$PINNED_COMMIT" ||
    fail "commit $PINNED_COMMIT not found after clone"
fi

# Hash verification: never continue from an unexpected tree.
actual="$(git -C "$DEST" rev-parse HEAD)"
[ "$actual" = "$PINNED_COMMIT" ] ||
  fail "r-source is at $actual, expected pinned $PINNED_COMMIT"
echo "Pinned upstream commit verified: $PINNED_COMMIT"

if [ -f "$DEST/VERSION" ]; then
  version="$(tr -d '\n' < "$DEST/VERSION")"
  nick="$(tr -d '\n' < "$DEST/VERSION-NICK" 2>/dev/null || true)"
  echo "R version: $version${nick:+ ($nick)}"
fi
echo "Trunk oracle for parity runs: r79999 (bac583951b728e97b9786804d3b4081f0fe18df5)"
