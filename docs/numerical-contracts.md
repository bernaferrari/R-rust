# Numerical Contracts for the FMA-Affected Kernels

This document records the two-mode comparison doctrine and the per-operation
numerical contracts for every kernel in the tree that contains deliberate
fused multiply-add (FMA) contraction sites. It is the written counterpart of
the portable property tests in `crates/rmath/tests/numerical_contracts.rs`,
which are the executable acceptance criteria for the **portable** mode.

## The two modes

1. **Portable numerical compatibility — the default product contract.**
   Any conforming build of rport (fused or unfused scalar arithmetic, any
   platform, any libm within the port's platform matrix) must satisfy the
   *numerical properties* of each operation: monotonicity, d/p/q identities,
   tail behavior, residual orthogonality, convergence status, and parameter
   estimates within operation-specific statistical tolerance. Acceptance is
   expressed as property bounds calibrated to pass under 1-ulp input
   differences but tight enough to catch real defects (wrong branch, lost
   rank, non-convergence, sign errors). These bounds live in
   `crates/rmath/tests/numerical_contracts.rs` and must hold in `--release`
   on every supported target.

2. **Exact-build reproduction — pinned-oracle evidence, narrower.**
   For some kernels we additionally contract to reproduce the *exact*
   rounding path of the pinned GNU R build
   (`r-source/` at GNU 4.7.0-dev, oracle `Rscript` at
   `~/.cache/rport/r-oracle/bac5839…/`): identical iterates, identical
   evaluation counts, bit-identical printed output. This is evidence about a
   specific compiler/flags pairing (clang/arm64 `-O2` with default
   `-ffp-contract=fast`, gfortran for the Fortran kernels), not a promise
   that holds across platforms. It is enforced by the exact-oracle
   conformance corpus (1181/1181 strict, `scripts/conformance_parity.sh`)
   and by in-module golden tests (e.g. the rbeta Marsaglia-MultiCarry stream
   test in `crates/nmath/src/dist/beta.rs` at 1e-13 relative tolerance, and
   the vmmin Rosenbrock count test in `crates/rmath/src/appl/optim.rs`
   asserting `(99, 44, 0)` fn/grad counts).

The rule of thumb: **FMA contraction is part of the exact-build contract
only where the pinned oracle's rounding path is observable** (iteration
counts, bit-exact cancellation, RNG streams). It is *never* part of the
portable contract; the portable tests must pass with contraction removed.

## Per-operation contracts

| Kernel | Intended algorithm | FMA in contract? | Portable acceptance criterion | Exact-build evidence |
|---|---|---|---|---|
| `vmmin` (BFGS, `optim(method="BFGS")`, `crates/rmath/src/appl/optim.rs`) | Nash's compact variable-metric BFGS with cubic-free line search (ACCTOL acceptance, gradient projection restarts) | Yes for exact-build: the oracle's contracted line-search rounding changes the Rosenbrock path (120 vs 118 fn evals); the checked-in GNU count contract pins it (commit `1970dfda`, de-fuse in `43208ded` reverted). No for portable. | Convergence code `fail==0`; objective strictly decreases from start; returned point within 1e-4 of the analytic Rosenbrock optimum `(1,1)` with `fmin < 1e-10`; independently computed gradient 2-norm `< 1e-3` at the result; exact quadratic recovered to 1e-8; `maxit=0` short-circuit returns the starting value. See `bfgs_*` tests. | In-module Rosenbrock count test (99 fn / 44 grad evals, captured from GNU vmmin with identical callbacks, reltol 1e-8); `arima0_public_contract` fixture; arima/StructTS/ts_smooth/loglin conformance set 103/103 (commit message of `1970dfda`). |
| `lbeta` tails (`crates/nmath/src/special/lbeta.rs`) | lgamma with `lgammacor` correction; asymptotic branches for large p/q | Yes for exact-build on the asymptotic branches: the clang/arm64 build contracts each `a*b+c`; mirrored with explicit `fma` so large-argument results match bit-for-bit. No for portable. | Log-beta consistency: `lbeta(a,b) == lbeta(b,a)`; `dbeta` integrates to 1; d/p/q round-trips (below) hold with tolerances that absorb a 1-ulp lbeta change. | `98cb1b73` introduced the contraction ("lbeta FMA contraction") with the qbeta.c exact-port wave; corpus `d-p-q-r-tests` / `d-p-q-r-tst-2` drivers PASS strict (1181/1181). |
| `qbeta` tails (`crates/nmath/src/dist/beta.rs`, exact port of `qbeta.c` incl. `DBL_MIN_EXP` control flow and MaybeSwap) | AS 109/Brackets + Newton refinement on the logit scale, switching on `DBL_MIN_EXP` scaling near the boundary | The tail refinements inherit lbeta's contracted values, so contraction matters for exact-build only; the portable contract is tail *behavior*, not bits. | Tail monotonicity in p for both tails (grids to p = 1e-12 and 1-1e-12); round-trip `pbeta(qbeta(p)) ≈ p` with tolerance derived from conditioning (`1e-12 + q·dbeta(q)·1e-9`); `log.p` entry agrees with plain entry to 1e-9 relative; closed-form references `Beta(1,1)≡p`, `Beta(a,1)=p^(1/a)`, `Beta(1,b)=1-(1-p)^(1/b)`, symmetric median ≡ 0.5; endpoints p=0/1 → 0/1; shape extremes to 1e10 stay finite and inside the support; out-of-range p → NaN; `dbeta` is the central-difference derivative of `pbeta`. See `qbeta_*` / `dbeta_*` tests. | Corpus cases `108_qbeta_numeric`, `517_qbeta_pbeta_roundtrip` (strict goldens); `demos2/p-qbeta` last-ulp codegen residual documented as platform-dependent in `98cb1b73`. |
| `ddot` / `daxpy` in LINPACK QR (`crates/rmath/src/appl/linpack_qr.rs`) | Reference BLAS semantics; LINPACK `dqrdc2`/`dqrsl` pivoted Householder least squares as used by `lm`/`Cdqrls` | Yes for exact-build: GNU builds the reference BLAS with `-ffp-contract=fast`; identical inputs (e.g. a model-matrix column in both x and y) must cancel *exactly*, which requires the fused `dy += da*dx`. No for portable. | Rank recovery on known-rank designs (full rank → k=3; exact collinearity → k=2 at tol 1e-7); residual orthogonality `|x_j' r| ≤ 1e-10·‖x_j‖·‖r‖·n` for every column; fitted+residual reproduces y to 1e-12 relative; coefficients of an exact system recovered to 1e-10 with machine-level residual (`‖r‖ ≤ 1e-12‖y‖`); noisy-design coefficients within 0.15 of truth. See `lm_qr_*` tests. | Introduced in `43208ded` ("dummy.coef via contracted ddot/daxpy") as part of closing the 91 conformance failures; `lm-tests` driver PASS strict; corpus 1181/1181 (CHANGELOG 2026-10-11 wave). |
| `uncmin` / `nlm` sites (`crates/rmath/src/appl/uncmin.rs`: ddot/dnrm2 accumulators, Householder `dsteps`, line-search `xpls = lambda·p + x`) | Gay's UNCMIN optif9 as wrapped by GNU `nlm` | Yes for exact-build at the listed sites: gfortran -O2 (Rblas) contracts each accumulate; the `xpls` update is the clang contraction that makes iterates bit-exact whenever lambda is not a power of two. One documented residual: a final un-replicated contraction leaves ~1e-8 relative drift in terminal nlm values (identical at R display precision; code and iteration count exact — see in-module `nlm_rosenbrock_matches_gnu`). No for portable. | Portable convergence properties follow the same shape as the vmmin contract (failure code, decrease, gradient norm at result, optimum within statistical tolerance); exercised end-to-end by the `nlm` conformance cases (`655_nlm`, `656_nlminb` PORT neighbor). | Commit `98cb1b73` ("uncmin optif9 engine fixed and FMA-aligned to the oracle build"); in-module `nlm_rosenbrock_matches_gnu` golden with the ~1e-8 residual documented. |

## Reference matrix

The external review asked for a four-row reference matrix. Status against
what exists in the repo's harness history (the `/tmp` scratch harnesses are
gone; the surviving evidence is in the checked-in tests, fixtures, CHANGELOG
entries and commit messages cited above):

| Reference row | Exists? | Where / what would need building |
|---|---|---|
| **Pinned GNU executable** (oracle `Rscript`, same fixtures) | **Exists.** | `scripts/conformance_parity.sh` runs both engines against goldens — 1181/1181 strict (CHANGELOG, 2026-10-11 wave). Oracle pinned by `scripts/install_r_oracle.sh` / `validate_r_oracle.py`. |
| **Same GNU source, contraction disabled** (`-ffp-contract=off`) | **Not built.** | The portable tests in `crates/rmath/tests/numerical_contracts.rs` are the standing surrogate: they are written to pass with contraction removed, so any conforming unfused build satisfies the portable contract. Building the actual unfused oracle (clang `-ffp-contract=off` recompile of `r-source/nmath` + `appl`) would turn rows 1–2 into a direct A/B; no such artifact is checked in. |
| **Another build of GNU R** (different compiler/platform) | **Partially.** | The rbeta golden test cites stock homebrew R 4.6.1 (clang default contraction) as a second binary at 1e-13 relative tolerance; `98cb1b73` classifies BLAS/LAPACK sessionInfo lines and the demos2/p-qbeta last-ulp codegen residual as platform-dependent in the comparison policy. No systematic cross-build harness exists. |
| **Independent high-precision / invariant check** | **Exists (invariants), partial (high precision).** | Invariant checks are now the portable test file (round-trips, monotonicity, orthogonality, closed-form references). Historical invariant evidence: `517_qbeta_pbeta_roundtrip` conformance case, `627_optim` Rosenbrock case. An arbitrary-precision (e.g. MPFR/Rational) cross-check of qbeta tails is not present and would need building. |

## Running the acceptance tests

```sh
cargo test -p rmath --release --test numerical_contracts
```

The exact-oracle corpus (which these tests deliberately do not duplicate)
remains the gate for mode 2:

```sh
./scripts/conformance_parity.sh --check
```
