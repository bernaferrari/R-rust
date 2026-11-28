# loessf.f exact port — work in progress (do not commit as final)

## State
- `loessf.rs` (830 lines): exact port skeleton — ehg106 quickselect, ehg124
  tree build, ehg125 vertex propagation, ehg127 WLS fit, ehg128 Hermite
  blending, ehg137 leaf descent, loess_fit_exact driver, lowesc statistics.
- `linpack.rs`: dqrdc/qr_solve/qty/dnrm2.
- Test harness: `crates/r-stats/examples/loess_test.rs` — run with
  `cargo run --example loess_test` from crates/r-stats.
- Case data: x=seq(0,1,len=30), y=sin(5x)+cos(31x)/8, w=1+x.
- Golden: fitted[1,15,30] = -0.0044871,0.6536559,-1.0234164;
  trace.hat,one.delta,two.delta,s = 4.8325647,24.788964,24.474259,0.1365471.
- CURRENT BUG: fitted[1,15,30] = -1.7433592,1.8760953,-2.7016164 — badly
  wrong; trl=8.97 vs golden 4.83.

## Confirmed Fortran facts
- ntol = floor(n × cell) = floor(30 × 0.2) = 6 (lowesb: ifloor(iv(3)*wv(2)))
- fd input wv(3) = 0 (Calloc zeroed, never written) → diameter condition
  NEVER triggers → leaves purely count-driven ≤ 6 points
- nf = min(n, floor(n×span + 1e-5)) = floor(22.5+1e-5) = 22
- nvmax = max(200, n) — no capacity constraint here
- kernel = iv(20) = 1 (tricube); weights w = sqrt(rw × (1-(d/rho)^{3/2... }))
  exactly as ehg127 loop 7-8
- interpolate/exact path: lowesb(fit+diagl) → lowesl(ehg191 hat matrix) →
  lowesc(trl/deltas) → lowese(ehg133 fitted at data points)
- ehg124 split: m = int((l+u)/2) positions over pi permutation; tie-offset
  scan only matters with duplicate x
- ehg128 blending: cubic Hermite phi0/phi1/psi0/psi1, derivative terms scaled
  by cell width, recursion deepest-coordinate-first

## Suspects for the fitted-value bug (ordered)
1. ehg127 coefficient extraction — dqrdc pivot/un-permute interaction;
   verify intercept extraction against a normal-equations reference fit
2. ehg106 quickselect port — the pi rearrangement indexing (l/r/j swaps)
3. ehg125 vertex propagation connectivity for d=1 — c arrays may place the
   new shared vertex in wrong corner positions
4. ehg128 corner ordering — c(1,leaf)/c(2,leaf) may not be lo/hi ordered
5. lf array layout inconsistent (hat matrix stats only, not fitted values)

## Remaining after fit fix
- symmetric family robustifying iterations (fit g): lowesw bisquare weights
  cmad=6×median|resid|, iterate control$iterations=4 with interpolate/none
- direct surface (ehg136 per-point exact fits)
- predict + se.fit (lowesp/lowesb + covar; s = sqrt(sum rw×res²/(delta1²/delta hmm — check predict.loess R code))
- subset/model args in loess_primitive; x data frame in model
- predict S3 generic dispatch (case tail: predict.example local method errors
  "no method for this object")
- tail features: z[]<-0 matrix assignment, sapply of new.env (both verified
  WORKING already)

## Sources
- Fortran: /tmp/loessf.f (R trunk), /tmp/dloess (netlib bundle — ehg128 at
  line 2545+, ehg176) — re-fetch if cleared:
  curl -sfL https://svn.r-project.org/R/trunk/src/library/stats/src/loessf.f
  curl -sfL https://netlib.org/a/dloess
- C orchestration: /tmp/loessc.c

## Debugging progress (cycle 2)
- ehg127 isolated at q=0: gives -1.7489322, naive normal-equations WLS gives
  -0.0045975 ≈ golden -0.0044871 → the naive math is right; the dqrdc/qr_solve
  LINPACK port is BROKEN.
- Trivial test dbgqr.rs: X=[1 1;1 2;1 3], y=(1,2,3) expect coef (0,1), got
  (-0.7002, 0.2437). jpvt=[2,1] correct, R diag=-3.7417 correct.
- HAND-VERIFIED correct so far: Householder scaling of first column,
  reflection application, qty for j=0 gives exactly (-3.7417, 0, 0).
- SUSPECTS: (1) second Householder step or its qty application; (2) the
  back-substitution indexing; (3) the qty loop `ju = k.min(n-1)` — LINPACK
  applies min(k, n-1) reflections hmm but should be ju=k reflections hmm —
  actually LINPACK dqrsl uses ju=min(k,n-1) for qty — verify against LINPACK
  reference implementation exactly. TEST NEXT: print intermediate qy after
  each reflection and R matrix entries, compare against hand computation or
  reference LINPACK (netlib linpack/dqrdc.f).
- Once qr path fixed: rerun loess_test expecting fitted ≈ golden, then wire
  loess_primitive to the exact fit, then symmetric/robustify + predict/se +
  direct surface + case tail.


## BREAKTHROUGH (end of cycle 2) — fit core BYTE-EXACT
- LINPACK bug found: dqrsl temporarily restores x(j,j)=qraux(j) during qty
  application — the packed R diagonal is NOT the Householder component.
  Fixed in both qr_solve and qty. Trivial system and full loess now exact.
- VERIFIED: fitted[1,15,30] = -0.0044871,0.6536559,-1.0234164 == golden
  trl/delta1/delta2 = 4.8325647,24.788964,24.474259 == golden exactly.
- The interpolate/exact fit path (gaussian family) is DONE numerically.

## Next steps in order
1. Wire loess_primitive (crates/r-stats/src/lib.rs:539) to loess_fit_exact:
   object components n, fitted, residuals, trace.hat=trl, one.delta, two.delta,
   s. s formula — CHECK loess.R: s = sqrt(sum(robust.weights hmm — actually
   read R loess source: s = sqrt(sum(weights * resid^2)/ ...) hmm —
   golden s = 0.1365471 — derive empirically: residuals available, weights
   w=1+x — try candidates.
2. predict: newdata points through ehg128 interpolation directly (fitted
   surface) — golden fit at .2,.5,.8 = 0.8439223,0.5937794,-0.6914726.
   se.fit needs the covariance machinery — read lowesp/ehg192 + R predict.loess
   se formula: se = s * sqrt(leverage-ish) — check loessc.c loess_predict.
3. symmetric family: robustifying iterations — lowesr residuals at data
   points (ehg192), lowesw bisquare weights, iterate control$iterations=4,
   statistics none during iterations (interpolate/none path), final fit with
   statistics. golden g fitted: -0.0025305,0.6509787,-1.0344803.
4. direct surface: ehg136 per-point exact fits — no tree interpolation.
5. loess_primitive args: subset, model, family, control wiring; x/model
   components for h$n/nrow(h$model)/h$x[1,1]/h$x[31,1] = "31 31 1 0".
   NOTE subset=c(30:1,1) — reorders! model=TRUE stores model frame.
6. predict S3 generic dispatch — case tail errors "no method for this object":
   predict must UseMethod-dispatch to predict.example in local scope.
7. Case tail verified working: z[]<-0, sapply new.env.
8. Delete old approximation loess_core_fit once wired; run sweep invariant:
   RUST_MIN_STACK=33554432 scripts/cargo_dev.sh test --lib conformance_sweep_cross_world -- --ignored --nocapture
   must hold pass: 496 output-mismatch: 0. Probe: PROBE_CASE=570_loess_contracts
   scripts/cargo_dev.sh test -p r-runtime --test probe_one
