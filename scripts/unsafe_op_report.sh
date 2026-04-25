#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_JSON="$(mktemp "${TMPDIR:-/tmp}/rport-unsafe-op.XXXXXX.jsonl")"
trap 'rm -f "$TMP_JSON"' EXIT

cd "$ROOT_DIR"

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi
if [[ "$RUSTFLAGS_FOR_BUILD" != *"--force-warn unsafe-op-in-unsafe-fn"* ]]; then
    RUSTFLAGS_FOR_BUILD="$RUSTFLAGS_FOR_BUILD --force-warn unsafe-op-in-unsafe-fn"
fi

env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" \
    cargo check -p rmath --message-format=json > "$TMP_JSON"

python3 - "$TMP_JSON" <<'PY'
import json
import sys
from collections import Counter

marker = "rmath-rs/rmath/src/"
counts = Counter()

with open(sys.argv[1], encoding="utf-8") as fh:
    for line in fh:
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("reason") != "compiler-message":
            continue
        message = event.get("message") or {}
        code = message.get("code") or {}
        if code.get("code") != "E0133":
            continue
        spans = message.get("spans") or []
        if not spans:
            continue
        file_name = spans[0].get("file_name", "")
        if marker in file_name:
            counts[file_name[file_name.index(marker):]] += 1

print("unsafe_op_in_unsafe_fn warnings by rmath module")
print("================================================")
if not counts:
    print("No warnings found.")
else:
    for file_name, count in counts.most_common():
        print(f"{count:5d}  {file_name}")
    print("================================================")
    print(f"{sum(counts.values()):5d}  total")
PY
