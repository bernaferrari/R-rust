#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${RPORT_R_ORACLE_MANIFEST:-$ROOT_DIR/oracle/r-oracle.json}"

fail() { echo "ERROR: $*" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || fail "python3 is required"
python3 "$ROOT_DIR/scripts/validate_r_oracle.py" --manifest "$MANIFEST"

IFS=$'\t' read -r COMMIT ARCHIVE_URL ARCHIVE_SHA256 < <(python3 - "$MANIFEST" <<'PY'
import json
import sys
from pathlib import Path

manifest = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
source = manifest["source"]
print(source["commit"], source["archive_url"], source["archive_sha256"], sep="\t")
PY
)
PREFIX="${RPORT_R_ORACLE_PREFIX:-$HOME/.cache/rport/r-oracle/$COMMIT}"
MARKER="$PREFIX/.rport-oracle-manifest.sha256"

sha256_file() {
    python3 - "$1" <<'PY'
import hashlib
import sys
from pathlib import Path

print(hashlib.sha256(Path(sys.argv[1]).read_bytes()).hexdigest())
PY
}

EXPECTED_MARKER="$(sha256_file "$MANIFEST")"
CONFIGURE_ARGS=()
while IFS= read -r argument; do
    CONFIGURE_ARGS+=("$argument")
done < <(python3 - "$MANIFEST" <<'PY'
import json
import sys
from pathlib import Path

manifest = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
print(*manifest["build"]["configure_args"], sep="\n")
PY
)

if [[ -x "$PREFIX/bin/Rscript" && -f "$MARKER" ]] &&
   [[ "$(tr -d '\n' < "$MARKER")" == "$EXPECTED_MARKER" ]]; then
    python3 "$ROOT_DIR/scripts/validate_r_oracle.py" --manifest "$MANIFEST" --runtime "$PREFIX/bin/Rscript"
    echo "$PREFIX/bin"
    exit 0
fi

BUILD_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/rport-r-oracle.XXXXXX")"
trap 'rm -rf -- "$BUILD_ROOT"' EXIT
ARCHIVE="$BUILD_ROOT/r-source.tar.gz"
command -v curl >/dev/null 2>&1 || fail "curl is required"
curl --fail --location --retry 3 --output "$ARCHIVE" "$ARCHIVE_URL"
ACTUAL_SHA256="$(sha256_file "$ARCHIVE")"
[[ "$ACTUAL_SHA256" == "$ARCHIVE_SHA256" ]] ||
    fail "R source archive SHA-256 is $ACTUAL_SHA256, expected $ARCHIVE_SHA256"

tar -xzf "$ARCHIVE" -C "$BUILD_ROOT"
SOURCE_DIR="$BUILD_ROOT/r-source-$COMMIT"
[[ -x "$SOURCE_DIR/configure" ]] || fail "archive did not contain expected source directory $SOURCE_DIR"
# GitHub commit archives intentionally contain no .git/.svn metadata. GNU R's
# build refuses to guess the revision, so provide the exact revision already
# bound to this commit by the validated manifest.
python3 - "$MANIFEST" "$SOURCE_DIR/SVNINFO" <<'PY'
import json
import sys
from pathlib import Path

manifest = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
source_date = manifest["runtime"]["source_date"]
svn_date = source_date.replace("T", " ").removesuffix("Z") + " +0000"
Path(sys.argv[2]).write_text(
    f"Revision: {manifest['runtime']['svn_revision']}\n"
    f"Last Changed Date: {svn_date}\n",
    encoding="utf-8",
)
PY

mkdir -p "$PREFIX"
(
    cd "$SOURCE_DIR"
    ./configure \
        --prefix="$PREFIX" \
        "${CONFIGURE_ARGS[@]}"
    make -j"${RPORT_R_ORACLE_JOBS:-2}"
    make install
)
printf '%s\n' "$EXPECTED_MARKER" > "$MARKER"
python3 "$ROOT_DIR/scripts/validate_r_oracle.py" --manifest "$MANIFEST" --runtime "$PREFIX/bin/Rscript"
echo "$PREFIX/bin"
