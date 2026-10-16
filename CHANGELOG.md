# Changelog

## 0.1.0 - Unreleased

This is an experimental, embeddable Rust R runtime. Entries are curated;
compatibility claims are defined by the maintained docs and executable checks.

### LOESS edge contracts completed (2026-10-16)

- The owned LOESS kernel now reproduces GNU's warn-and-continue semantics
  for degenerate neighborhoods instead of erroring: duplicate/zero-span
  neighborhoods, all-zero weights and underdetermined (`k > nf-1`) local
  models emit the full ehg127 diagnostic stream (`span too small…`,
  `at`/`radius`/`all data on boundary…`, `pseudoinverse used at`,
  `neighborhood radius`, `reciprocal condition number`, `There are other
  near singularities as well.`, `zero-width neighborhood…`) with GNU's
  `%.5g` formatting, emission order and cumulative sing/rcond state, and
  flush to GNU's outcomes (zero coefficients, NaN coefficients for
  underdetermined zero-weight neighborhoods, `enp = n - delta1` via
  ehg141's z-clamp at zero trace). Diagnostics are recorded in a warning
  buffer on `loess::Execution` and replayed as R warnings by the `.C`
  bridge and the portable fit path; documented residuals (LINPACK rounding
  debris in rcond digits, zero-width vertex presence on identical
  predictors) live in docs/numerical-contracts.md.
- `getCurrentCall` now applies GNU's rule that bytecode-expression
  attribution applies only when no function frame is on the stack, so
  warnings raised inside `.C`-bridged numerics attribute to the enclosing
  closure (e.g. `In simpleLoess(...)`), matching the oracle byte-for-byte.
- New oracle-verified regressions in crates/r-embed/tests/loess.rs for
  call/terms metadata (model.frame, update, predict newdata, `(weights)`
  naming), degenerate flush outcomes, and the singular-neighborhood
  warning stream.


### Loader-independent base datetime/format/sink bindings (2026-10-12)

- The four conformance failures exposed by running with the upstream base
  loader gated off are fixed by binding the GNU-verbatim R closures the
  flip had left provider-less in the default configuration:
  `as.POSIXlt.POSIXct` (strptime's ±Inf path calls it by name; the
  `.Internal(as.POSIXlt(x, tz))` body cannot recurse into the generic the
  way the old deleted version did), the datetime Ops group
  (`Ops.difftime`, `*.difftime`, `/.difftime` — mixed-unit difftime
  arithmetic coerces through coerceTimeUnit + NextMethod, keeping the
  arithmetic primitive a raw `.default` backend) together with
  `+.Date`/`-.Date`/`+.POSIXt`/`-.POSIXt`/`.check_tzones` (required so
  Ops.difftime yields to the Date/POSIXt methods under GNU's dispatch
  precedence), and `formatC` (base/format.R verbatim; the builtin keeps
  only util.c's 7-argument `.Internal(formatC(...))` convention). All of
  them shadow the loader's identical whitelist copies, so both
  configurations dispatch identically.
- `sink(file)` now builds its internal `file()` call with GNU sink.R's two
  positional arguments (description, open), so blocking (position 2) and
  encoding (position 3) keep their defaults after the `file()` argument
  order fix.
- The arithmetic primitive's difftime comparison no longer rescales by the
  units label: GNU's C comparison is raw because the R-level Ops.difftime
  already coerced both operands to seconds before NextMethod (visible as
  `1 min > 100 secs` now correctly printing FALSE).

### Projection-pursuit regression fidelity fixes (2026-10-10)

- `ppr` (`smart.rs`, the `ppr.f` SMART port) now matches GNU R bit-for-bit on
  the supsmu path, verified against a `gfortran` build of the vendored
  `ppr.f` across ten probe fits (p up to 3, q up to 3, term dropping,
  non-uniform weights, duplicate abscissae, `optlevel` 0/1/2). Four
  translation defects fixed: `oneone` sorted the stale scratch column instead
  of the projection column before `supsmu` (scrambling the stored term
  curves, e.g. `ppr(1:5, c(1.1,1.9,3.2,3.8,5.1), nterms=1)` fitted
  `1.8,2.41,3.02,3.63,4.24` instead of `1.04,2.03,3.02,4.01,5`);
  `smart1` read the weakest-term index from the sort-key column instead of
  the permutation column; `newb` wrote initial ridge loadings into column 1
  instead of column `lm` (zeroing the projection response of every term
  after the first); and `ppconj`'s packed off-diagonal index was
  `i*(i-1)/2+j` instead of `i*(i+1)/2+j`, corrupting direction searches for
  two or more predictors.

### Full-parity conformance and driver wave (2026-10-11)

- The exact-oracle conformance corpus now passes 1181/1181 strict (was 1090/91):
  bound the missing stats natives (HoltWinters/STL/supsmu/isoreg/Ansari/nlminb/
  Smirnov/BinDist) and ported ks.c, loglin.c (AS51 plus the GNU wrapper), the
  PORT nlminb contract and the ppr.f SMART engine; fixed the L-BFGS-B kernel
  (dtrsl/cauchy/matupd/formk/formt/subsm), arima getQ0, KalmanSmooth nit,
  smooth.spline stxwx boundary, dummy.coef via contracted ddot/daxpy, and
  GNU-exact scalar real formatting; `match.arg`/`format(justify=)`, `.Defunct`,
  marginSums, exists() base fallback; Date/POSIXt/difftime/strptime parity.
- The native graphics surface is complete: C_clip and C_convertX/Y plus persp,
  filledcontour, path, symbols, xspline, dend, dendwindow, erase, locator,
  identify - real device-level clipping with display-list replay, GNU
  null-device semantics for device-less builds.
- `unserialize(connection)` consumes exactly one object byte-exactly with
  pull-mode admission (short malicious headers rejected before allocation);
  namespace S3-methods tables are created pre-lock so getNamespace/asNamespace
  work on loaded packages.
- Twelve of the twenty failing whole upstream drivers now pass against the
  pinned GNU oracle (classes-methods, both d-p-q-r, datasets, datetime2/4/5,
  iconv, lm-tests, r-strict-tst, reg-plot-latin1, utf8); La_library honestly
  reports the faer backend and BLAS/LAPACK sessionInfo lines are classified
  as platform-dependent in the comparison policy.

### Browser, graphics and repository organization (2026-09-08)

- Added the Rove website with sixteen runnable examples, focused editor pages,
  light/dark themes, and browser-model or Ollama code drafting.
- Bounded the browser R arena and result export, with a compiled 256 MiB Wasm
  linear-memory maximum and recovery tests.
- Added persistent grid viewport navigation, mixed-unit expressions and live
  grid snapshot restoration; validated plotmath metrics against same-font GNU R
  fixtures at three sizes and bundled real bold/oblique font faces.
- Grouped the workbench under `apps/workbench` and the minimal Android
  integration under `examples/android-compose`. Archived historical audits
  and plans, and kept generated output under `target/`.
- Standardized the GPL license text for GitHub discovery and included the
  license texts in release bundles.

### Numerical fidelity fixes (2026-08-29)

- `besselJ` now matches GNU R trunk for integer orders >= 3 at small and
  moderate `x`: the branch-3 storing backward-recursion loop reads the
  `b[n+2]` term (was reading `b[n+1]`), the `nend < 0` path falls through
  into the shared storing loop like `J_bessel` in `bessel_j.c` instead of a
  divergent re-derivation, normalization uses `Rf_gamma_cody` like stock, and
  the `very_small_nu` clamp is `2^-800` (was `2^-1022`). 64-probe trunk
  parity matrix added (`rmath-test` Special Functions).
- `sample(..., prob = ...)` with replacement now uses Walker's alias method
  (port of `walker_ProbSampleReplace` in `random.c`) when more than 200
  categories have `n * p[i] > 0.1`, so the sampled values match stock R
  exactly on a seeded stream (previously those wide weight vectors took the
  plain path with a different draw count). Seeded-parity tests added
  (`rmath-test` Sampling incl. the 200/201 dispatch boundary and the
  `sample.kind = "Rounding"` variant).

### Correctness and maintenance fixes (2026-07-11)

- Corrected two TOMS 708 translation defects: `bup` now retains every
  increasing-series term and `bfrac` advances its recurrence state like GNU R.
- Beta distribution wrappers now honor `log`, `lower.tail`, and `log.p`; added
  differential fixtures for beta/F branches, tail/log behavior, and roundtrips.
- Restored `$` fallback evaluation to GNU R's sequencing, fixing environment
  lookup, active bindings, locked bindings, and `sys.source(..., envir=...)`.
- Removed the obsolete UniFFI 0.28 dependencies and generated Kotlin binding
  from `rmath`; Android bindings are owned exclusively by `r-uniffi` 0.30.
- Continued `essentials.rs` decomposition by extracting environment binding and
  locking builtins behind stable re-exports.
- Updated the upstream source map after the `dist` to `nmath::dist`
  consolidation.

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
