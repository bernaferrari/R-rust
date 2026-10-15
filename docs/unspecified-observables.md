# Unspecified-order observables

Classification of exact-build divergences that are not semantic contract
violations. Companion to the architecture doctrine: portable numerical
compatibility (operation-specific tolerances and invariants) is the product
contract; exact-build reproduction against the pinned oracle is separate,
narrower evidence.

## Gzip compressor block choices

Gzip files now use the Rust zlib implementation on native, Android and Wasm.
Their GNU R container metadata (zero timestamp and flags, no filename, Unix
OS byte) and decoded bytes are preserved. Deflate block choices and compressed
file sizes can differ from a particular GNU R system-zlib build. This includes
incompressible byte vectors at low compression levels; byte-identical compressed
files are not claimed. R serialization inside the container is unchanged.

The independent pinned GNU fixtures in
`crates/rmath/src/mainutils/zlib/gnu-gzip.tsv` cover every compression level,
empty/text/raw inputs and inputs larger than a deflate window. Tests require
exact header and decoded-payload agreement for both implementations. Original
whole-driver comparisons retain their existing exact-build dispositions.

## Global-environment enumeration order (`ls()` / `as.list(.GlobalEnv)` unsorted)

GNU R documents the unsorted enumeration order of environment contents as
arbitrary: it depends on the internal hash-table state, which tracks
first-bind times and rehash growth. Two sessions that bind the *same* names
to the *same* values can therefore enumerate them in different orders —
e.g. loop variables rebound after their first bind keep their original hash
slot, so a build whose insertion history differs places them one slot away.

**Case: `tests/upstream-r/vendor/datetime3.R`.** The driver currently fails
exact-build comparison only on the order of `as.list(.GlobalEnv)` names:
3 of 212 names sit one slot differently because their first-bind times
differ (rebound loop variables). This is an *unspecified-order observable*:

- The **semantic contract** is the association set (every name bound to the
  right value), the locale/C-ordered `ls(sorted = TRUE)` listing, the
  binding kinds (active vs. ordinary), and order-insensitive membership of
  the unsorted enumeration. This is covered by
  `crates/rmath/src/sexp/envir/global_env_enumeration_tests.rs`, whose
  golden output is byte-identical to the pinned oracle build (r90451) on a
  representative history (assignments, loop rebinds, `rm()` + re-add, an
  active binding).
- The **exact-build disposition** of the driver is unchanged: it remains
  exact-build evidence and is NOT claimed passing. See
  `tests/upstream-r/dispositions.tsv`.
