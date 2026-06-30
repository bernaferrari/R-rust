# Changelog

## 0.1.0 - Unreleased

This is the first Android-focused Rust R runtime slice.

### Fidelity fixes (2026-06-30)

- `pbeta` / `pbeta_raw` now call TOMS 708 `bratio` (`nmath::special::toms708`), matching stock R `pbeta.c`.
- `R_DispatchOrEvalSP` in subset/subassign evaluates args and dispatches S3/S4 via `DispatchOrEval` (was always false).
- `ALTREP_CHECK` honors the SEXP alt bit (was hardcoded false).
- `is.loaded` delegates to `dotcode::do_isloaded` / `R_lookupLoadedSymbol` (was always FALSE).
- Corrected `dbeta(0.5,2,5)` unit expectation to stock R `0.9375` and enabled the test.
- Port map: beta/toms708 paths updated; known-gap rows for ALTREP depth, bytecode, grid, graphics.
- CI: clippy on embed crates, strict conformance + safe API audit on Linux.

### Supported Highlights

- Session-owned R runtime with isolated arenas, environments, RNG state,
  protect/preserve stacks, output capture, path policy, resource limits, and
  cancellation.
- Rust embedding API through `r-embed` with owned `RValue`, `EvalOutput`,
  package metadata, runtime info, arena stats, and PNG plot bytes.
- UniFFI API through `r-uniffi` for Android/Kotlin hosts.
- Android Compose sample with two independent sessions, eval, package loading,
  S3 package showcase, plot rendering, and cancellation.
- Pure-R package discovery/loading from Android app-private library paths.
- Stock C R conformance harness currently passing 211/211 checked cases, plus
  5/5 curated upstream GNU R core slices.
- Release gates for formatting, strict clippy, focused Rust tests, Android
  aarch64 checks, global-state audit, conformance parity, safe API audit,
  upstream source-map validation, Android packaging, and performance snapshots.

### Upstream R Sync

Applied from `r-source` trunk (synced through 2026-06-26):
- PR#19055 — `d/p/q/rweibull` now accept `shape = 0` (degenerate Weibull),
  matching upstream guards and `R_P_bounds_01` for `pweibull`.
- PR#19069 — `aperm()` short-circuits identity permutations with `resize=TRUE`,
  returning the original array without copying.
- grid `allocationRemaining` returns `FALSE` for `initial == 0`.

Reviewed but not applicable this cycle: `datetime.c` const-correctness (N/A in
Rust), `localtime.c` `lcl_is_set` tri-state (already present in the Rust
`tzone` port), and `portsrc.f` `nlminb` aliasing fix (subroutine not yet ported).

### Known Limits

- This is not a full GNU R replacement yet; the conformance suite covers a
  credible but bounded subset.
- Native package loading through `useDynLib()` is intentionally rejected on
  Android until a host-owned native-library policy exists.
- Graphics support is a small headless plotting subset, not a complete R
  graphics device.
- Exact byte-for-byte `.Random.seed` stream parity is not currently claimed.
- Some translated internals remain C-shaped to preserve upstream reviewability;
  public embedding and Android APIs are Rust-shaped and owned.
