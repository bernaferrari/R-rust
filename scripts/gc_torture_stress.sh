#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# GC-torture stress (nightly): run a deterministic allocation-heavy R case
# through the conformance runner with gctorture(TRUE) armed — every
# allocation forces a full mark/sweep — and compare the normalized output
# with stock C R running the same case under the same torture. A GC bug
# shows up as dropped/corrupted values, so the differential catches
# collector damage, not just crashes. Not part of the PR CI bar.

if ! command -v Rscript >/dev/null 2>&1; then
    echo "SKIP: Rscript not found; GC torture stress differential requires stock C R." >&2
    exit 0
fi

if [[ "${RPORT_REQUIRE_PINNED_ORACLE:-0}" == "1" ]]; then
    python3 "$ROOT_DIR/scripts/validate_r_oracle.py" \
        --runtime "$(command -v Rscript)"
fi

RUSTFLAGS_FOR_BUILD="${RUSTFLAGS:-}"
if [[ "$RUSTFLAGS_FOR_BUILD" != *"-Awarnings"* ]]; then
    RUSTFLAGS_FOR_BUILD="${RUSTFLAGS_FOR_BUILD:+$RUSTFLAGS_FOR_BUILD }-Awarnings"
fi

echo "INFO: building Rust rmath artifact for the GC torture runner." >&2
(cd "$ROOT_DIR" && env RUSTFLAGS="$RUSTFLAGS_FOR_BUILD" cargo build -p rmath >/dev/null)

rust_rlibs=()
for candidate in \
    "$ROOT_DIR"/target/debug/deps/librmath-*.rlib \
    "$ROOT_DIR"/target/debug/deps/librmath.rlib \
    "$ROOT_DIR"/target/debug/librmath.rlib; do
    if [[ -f "$candidate" ]]; then
        rust_rlibs+=("$candidate")
    fi
done
if (( ${#rust_rlibs[@]} == 0 )); then
    echo "ERROR: Rust rmath artifact missing after build." >&2
    exit 1
fi
RUST_RLIB="$(ls -t "${rust_rlibs[@]}" | head -n1)"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/rport-gc-torture.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

RUST_BIN="$WORK_DIR/rust_runner"
cp "$RUST_RLIB" "$WORK_DIR/librmath.rlib"

if ! rustc --edition=2024 "$ROOT_DIR/tests/conformance/src/main.rs" \
    -L dependency="$ROOT_DIR/target/debug/deps" \
    --extern rmath="$WORK_DIR/librmath.rlib" \
    -o "$RUST_BIN" >"$WORK_DIR/rustc.log" 2>&1; then
    echo "ERROR: failed to compile Rust conformance runner"
    sed 's/^/  rustc | /' "$WORK_DIR/rustc.log"
    exit 1
fi

TORTURE_CASE="$WORK_DIR/gc_torture_case.R"
cat >"$TORTURE_CASE" <<'RCASE'
## GC torture stress case: deterministic allocation churn. A short
## gctorture(TRUE) section forces a collection on EVERY allocation; the
## bulk loops run under gctorture2(25) (collection every 25th allocation)
## so the stock-R differential stays within nightly budget. Nightly
## differential: stock C R and the Rust runner must print byte-identical
## normalized output. Uses only version-stable constructs: fixed seeds,
## integer-exact aggregates, no .Random.seed layout dumps, no current-time
## dependencies.
gctorture(TRUE)
set.seed(10)
for (i in 1:25) {
  x <- runif(30)
  if (i %% 5 == 0) invisible(gc())
}
gctorture(FALSE)
set.seed(10)
print(runif(3))

gctorture2(25)
set.seed(11)
hits <- 0L
for (i in 1:150) {
  x <- runif(50)
  lst <- lapply(1:20, function(k) list(v = x[k:(k + 9)], tag = paste0("t", k)))
  hits <- hits + length(lst)
  if (i %% 25 == 0) invisible(gc())
}
cat("hits ", hits, "\n", sep = "")

set.seed(12)
acc <- integer(0)
for (i in 1:100) {
  acc <- c(acc, sample(1:26, 3))
  if (i %% 20 == 0) invisible(gc())
}
cat("acc ", length(acc), max(acc), length(unique(acc)), "\n", sep = "|")

set.seed(13)
count <- 0L
m <- matrix(0, 40, 40)
for (i in 1:60) {
  m[sample(40, 8), sample(40, 8)] <- runif(64)
  env <- new.env(parent = emptyenv())
  for (j in 1:10) assign(paste0("k", j), rnorm(5), envir = env)
  count <- count + sum(m > 0)
  if (i %% 15 == 0) invisible(gc())
}
cat("count ", count, "\n", sep = "")

gctorture(FALSE)
set.seed(13)
print(rnorm(2))
cat("torture ok\n")
RCASE

norm() {
    tr -d '\r' | sed 's/[[:space:]]*$//' |
        awk '{ lines[NR] = $0 } END { n = NR; while (n > 0 && lines[n] == "") n--; for (i = 1; i <= n; i++) print lines[i] }'
}

echo "INFO: stock R pass under GC torture." >&2
if ! env LC_ALL=C LANG=C Rscript --vanilla "$TORTURE_CASE" >"$WORK_DIR/c.out" 2>&1; then
    echo "ERROR: stock R failed under GC torture"
    sed 's/^/  C | /' "$WORK_DIR/c.out"
    exit 1
fi

echo "INFO: Rust runner pass under GC torture." >&2
if ! env LC_ALL=C LANG=C "$RUST_BIN" "$TORTURE_CASE" >"$WORK_DIR/r.out" 2>&1; then
    echo "ERROR: Rust runner failed under GC torture"
    sed 's/^/  R | /' "$WORK_DIR/r.out"
    exit 1
fi

norm <"$WORK_DIR/c.out" >"$WORK_DIR/c.norm"
norm <"$WORK_DIR/r.out" >"$WORK_DIR/r.norm"

if ! cmp -s "$WORK_DIR/c.norm" "$WORK_DIR/r.norm"; then
    echo "ERROR: GC torture differential divergence (stock R vs Rust runner)"
    diff -u "$WORK_DIR/c.norm" "$WORK_DIR/r.norm" || true
    exit 1
fi

echo "GC torture stress passed: stock R and the Rust runner agree under GC torture."
