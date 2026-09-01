# Conformance Dashboard

`scripts/conformance_parity.sh` compares the Rust runtime against stock C R.
It is the release-facing proof that a checked-in fixture behaves the same in
both engines.

The harness:

- runs normal fixtures from `tests/conformance/cases/*.R`
- runs expected-error fixtures from `tests/conformance/error_cases/*.R`
- executes stock C R with `Rscript --vanilla`
- executes the Rust interpreter through `tests/conformance/src/main.rs`
- normalizes deterministic output and error text
- compares both engines against checked-in goldens
- treats `tests/conformance/xfail.tsv` as the owned known-gap list
- optionally writes machine-readable JSON and human-readable Markdown reports

## Commands

Run parity only:

```bash
./scripts/conformance_parity.sh --check
```

Run parity and write reports:

```bash
./scripts/conformance_parity.sh --check --report target/conformance-report
```

Run the release-grade gate:

```bash
./scripts/conformance_parity.sh --check --strict --report target/conformance-report
```

Strict mode is intentionally harder than local developer mode: it requires
`Rscript --vanilla` to be installed and fails if any `xfail.tsv` entry remains
active or unexpectedly passes. Local non-strict runs may still skip cleanly when
stock GNU R is unavailable.

That writes:

- `target/conformance-report/summary.json`
- `target/conformance-report/summary.md`

You can also choose explicit output paths:

```bash
./scripts/conformance_parity.sh \
  --check \
  --json target/conformance-summary.json \
  --markdown target/conformance-summary.md
```

Regenerate the goldens from stock C R (run manually; never part of CI —
goldens are reviewed artifacts, so commit the diff only after reading it):

```bash
./scripts/conformance_parity.sh --regen-goldens
```

It re-runs every `tests/conformance/cases/*.R` and
`tests/conformance/error_cases/*.R` under `Rscript --vanilla`, applies the
harness's normalization (plain output normalization for normal cases, error
normalization for expected-error cases), and rewrites
`tests/conformance/golden/*.out` and `tests/conformance/error_golden/*.out`
in place. It refuses to run without `Rscript` on `PATH` and leaves goldens
untouched whenever stock R misbehaves (a normal case exiting non-zero, or an
error case that stops erroring).

Golden provenance: the checked-in goldens are regenerated locally against a
GNU R trunk build (r90451, "R version 4.7.0 Under development (unstable)").
The required CI gate builds that exact source from commit
`bac583951b728e97b9786804d3b4081f0fe18df5`. Its machine-readable manifest is
`oracle/r-oracle.json`; validation requires a full commit, verifies the source
archive SHA-256, and records the manifest digest beside the installed runtime.
The harness rejects a required-oracle run when `Rscript` lacks that provenance
or reports a different runtime identity. Nightly `release` and `devel`
comparisons are explicitly informational drift probes and cannot replace or
weaken the exact-oracle result.

To install the same oracle locally, run `scripts/install_r_oracle.sh`, prepend
the printed directory to `PATH`, and set `RPORT_REQUIRE_PINNED_ORACLE=1` for a
provenance-enforced check.

## Upstream Core Slices

`scripts/upstream_core_slices.sh` first validates a mechanical import of the
complete 245-file `r-source/tests` tree from the exact pinned GNU R source
commit, including every file's SHA-256 and explicit pass/xfail/skip dispositions
for all 70 top-level `.R`/`.Rin` drivers. It then executes every whole-file
pass/xfail disposition and runs curated excerpts adapted from GNU R's
`r-source/tests/arith.R`, `arith-true.R`, `eval-etc.R`, `conditions.R`,
`any-all.R`, `structure.R`, `complex.R`, `primitives.R`, `eval-fns.R`,
`simple-true.R`, `print-tests.R`, `reg-IO2.R`, and `reg-tests-1b.R`.
Unlike the numbered conformance fixtures, these cases compare live stock
`Rscript --vanilla` output directly against the Rust runtime. They are intended
for evaluator/arithmetic regression work and the release gate:

```bash
scripts/upstream_core_slices.sh --strict --report target/upstream-core-slices
```

Known unsupported upstream expectations belong in
`tests/upstream-core/xfail.tsv` with an owner bead. Passing slices should remain
xfail-free. The unedited full files, immutable inventory, and owned disposition
ledger are under `tests/upstream-r`. A whole-file `pass` or `xfail` row is always
executed; XPASS is a failure. A `skip` row is permitted only with a valid owner
bead and a concrete reason.

If `Rscript` is missing, the default harness prints a deterministic skip message
after validating the imported corpus and returns `0`. `--strict` turns that into
an error and is mandatory in CI and release gates. Those gates additionally set
`RPORT_REQUIRE_PINNED_ORACLE=1`, so an arbitrary local R cannot satisfy parity.

## Pure-R Package Corpus

`scripts/pure_r_package_corpus.sh --check --report target/pure-r-package-corpus`
runs the Android-style pure-R package corpus and writes JSON/Markdown proof.
The corpus covers package metadata discovery, `library()` loading, namespace
exports/imports/S3 directives, DESCRIPTION `Depends`, `packageVersion()`/
`packageDescription()`, namespace-only loading, namespace-qualified access,
source-form package data, explicit `envir` loading, package resource/example
lookup through `system.file()`, DESCRIPTION `Collate` source ordering,
source-form `LazyData`, serialized data policy errors, same-name package
isolation across sessions, and explicit native-code or bytecode package
rejection. The release gate runs this script after the stock-R performance
comparison.

## Safety Testing

Behavioral parity is only half of the release claim; the ownership and
GC layer has its own gates. They run as nightly jobs in
`.github/workflows/nightly.yml` (`miri`, `gc-torture`) — deliberately
not part of the PR bar.

### Miri subset

`cargo +nightly miri test -p rmath sexp::` runs the `sexp::` safe-layer
test subset under Miri with Stacked Borrows checking in the default
permissive-provenance mode. The leak check is disabled
(`-Zmiri-ignore-leaks`) because the runtime deliberately allocates
immortal persistent objects — base symbols, `CHARSXP` payloads,
primitive metadata — that live until process exit, mirroring upstream
R; the aliasing check that is the point of the job is unaffected.

**216 tests are proven Miri-clean.** This is deliberately a bounded
claim: ~167 `sexp::` tests are not yet Miri-run, and the evaluator and
library layers have no Miri coverage at all. The nightly job re-runs
the subset as it grows. What the audit tested, found, and fixed is
documented in
[Object ownership and GC safety](rust-r-port-architecture.md#object-ownership-and-gc-safety).

### GC-torture stress

`scripts/gc_torture_stress.sh` (nightly job `gc-torture`) runs a
deterministic allocation-heavy R case through the conformance runner
with `gctorture(TRUE)` armed — every allocation forces a full
mark/sweep — and compares the normalized output with stock C R running
the same case under the same torture. The job builds against the exact
pinned R oracle and validates provenance with
`RPORT_REQUIRE_PINNED_ORACLE=1`, like the parity gates. A GC bug
surfaces as dropped or corrupted values, so the differential catches
collector damage, not just crashes.

### Compile-fail tests

Compile-fail coverage exists as doctests: `Sexp` carries a
`compile_fail` doctest pinning the non-`Copy` use-after-move contract
(a `Copy` handle would let a stale alias survive an in-place mutation
of the same R object). There is no trybuild/UI-test rig yet; broader
compile-fail coverage is open work alongside the planned
`SexpRef`/`SexpMut` borrow split.

By the same policy as parity numbers: the proven Miri subset is a
floor, not a whole-runtime claim. The release-facing safety stance
lives in the README's
[Safety status](../README.md#safety-status).

## Current Status

As of the latest local run:

| Metric | Count |
| --- | ---: |
| Total parity cases | 608 |
| Passing | 608 |
| Failing | 0 |
| Expected failures | 0 |
| Unexpected passes | 0 |

Current domain coverage:

| Domain | Passing Cases | Notes |
| --- | ---: | --- |
| Parser and scalar basics | 30 | Arithmetic, scalar values, comments, infix continuation, parse/deparse/dput/bquote/RDS/unname/expression/mode/storage-mode/tsp/comment-attribute/attr/attributes/dim/length-replacement, repetition helpers, tabulation, findInterval boundary options including single-break edges, serialize version/ascii/xdr header and payload handling including ASCII string escapes, shape-helper, array-creation, and broad parser/runtime smoke cases |
| Evaluator, closures, and control flow | 25 | Closures, lexical scope, lazy/default args, missing args, loops, and evaluator visibility/control-flow checks |
| Vectors, lists, attributes, and objects | 169 | Vectors, typed vector constructors, lists, names, name-preserving repetition, list/complex repetition, named and ordered mixed-list unlisting, recursive unlist control, typed rle/inverse.rle, raw-vector serialization roundtrips including ASCII hex-byte payloads, subsetting, factors, explicit missing factor levels, generated factors, factor coercion, factor summaries, interval cutting, ordered factors, ordered comparisons, interaction factors, releveling, droplevels, class/attribute replacement, matrices, data frames, S4 slots, and grouped object helpers |
| Base functions, conditions, and platform helpers | 178 | Sorting/set helpers, output capture, conditions, search-path/environment helpers, options, file/temp/path helpers, connections, platform state, `.Internal` dispatch, non-GNU alias absence semantics, live environment-variable read/write parity with `Sys.setenv`/`Sys.getenv` and `scan(text=)` parsing, and stock `as.character` double formatting at 15 significant digits with condition print rendering |
| Stats, math, and RNG | 124 | Numeric summaries, distributions, beta/F TOMS 708 branches, beta tail/log flags and quantile/CDF roundtrips, arithmetic edge cases, complex hyperbolics, `sample`/`sample.int`, pmin/pmax missing-value and character coercion semantics, typed cumulative extrema, typed cumulative sum/product NA semantics, typed `diff()` integer/logical and overflow semantics, array margin summaries, aggregate/tapply/by grouped summaries, and summary-vector parity |
| Packages, namespaces, and S3 | 10 | Package namespace and S3 fixtures, `system.file()`, S3 method-export absence, and the pure-R package corpus gate |
| Graphics and Android embedding | 4 | Base graphics layout state and external graphics dispatch smoke; broader graphics parity remains tracked separately |
| Error semantics | 67 | Missing argument, `stop`, `stopifnot`, sampling errors, matrix-helper validation, dmultinom validation, match/char-expand/sprintf/read.fwf/gcinfo/gctorture2/function-lookup expected errors, serialization input validation, internal dispatch errors, `relevel()` validation, parse errors rendered like Rscript (`unexpected ')' in "<context>"`, `unexpected end of input`), and selected platform expected errors |

The generated report is the source of truth for exact current counts. Do not
hand-edit release numbers without rerunning the report command.

The curated upstream slice gate currently passes 15/15 live stock-R comparison
cases with zero expected failures, including the `any-all.R` helper path through
`deparse(substitute(.))`, `do.call()`, list concatenation, named `na.rm`, and
`identical()`. The complex slice now covers parsed imaginary literals, complex
vector construction through `c()`, complex powers, `sqrt`, `exp`, `log`,
trigonometric functions, and the `Re`/`Im`/`Mod`/`Arg`/`Conj` primitive family.
The structure slice covers `structure()` attribute attachment, dotted stock-R
attribute remapping for `.Dim`, `.Dimnames`, and `.Label`, `attributes()` named
list results, and factor/class preservation.
The primitive slice includes base namespace-qualified primitive lookup through
`base::`, primitive classification, callable builtin resolution, per-session
`.ArgsEnv`/`.GenericArgsEnv` prototype metadata for `args()`, and the
`tools::langElts` language-element registry used by GNU R's primitive
accounting checks. The empty-vector formatting/IO slice covers zero-length
`format`, `format.info`, `noquote`, `nchar`, `nzchar`, path mapping, and
filesystem predicate/create behavior against live stock R.

## Status Policy

- `pass`: stock C R, checked-in golden output, and the Rust runtime agree after
  deterministic normalization.
- `fail`: behavior differs and must be fixed or added to `xfail.tsv`.
- `xfail`: accepted known gap with an owner bead and reason.
- `xpass`: stale expected failure; remove it from `xfail.tsv` or update the
  owner bead.

Release gates run with `--strict`, so `xfail` debt is not shippable. Temporary
expected failures are useful while developing a slice, but a release-quality
claim must either fix the behavior or explicitly remove the case from the gate.

`xfail.tsv` rows are tab-separated:

```text
case_id<TAB>owner_bead<TAB>reason
```

## Release-Gap Tracking

The parity suite is strong enough to catch regressions in the covered surface,
but it is not a full R compatibility claim. The original release-gap beads for
the Android-ready slice are closed:

- `rport-az2r`: base language parity top-50 gaps
- `rport-x3pp`: S3 release parity beyond the first registry slice
- `rport-dgn7`: stats, RNG, and numeric fidelity release slice
- `rport-ifek`: pure-R package corpus smoke test
- `rport-c6ap`: graphics path release quality
- `rport-89pz`: Android UniFFI release surface hardening

Broader GNU R compatibility still requires new scoped beads before it can be
claimed: full upstream test expansion, compiler/bytecode/lazyload depth, full
methods/S4 coverage, complete graphics/device behavior, and host/native package
policy. New behavior should land with a stock-R fixture whenever possible. If
exact stock-R parity is intentionally out of scope for Android, add a policy
note and an owner bead instead of silently broadening claims.

## Unsupported Surface Ledger

This ledger keeps the release claim explicit. It is not a substitute for bead
tracking; any row that moves from policy/status documentation into implementation
work needs a scoped bead and a stock-R or target-specific gate.

| Surface | Current stance | Owner bead | Proof or gate |
| --- | --- | --- | --- |
| Full GNU R compatibility | Not claimed by the Android release subset. All top-level upstream tests are now pinned and dispositioned; curated slices are green while the full files remain explicitly owned work. | `rport-2gpp.1` through `rport-2gpp.5` | `scripts/conformance_parity.sh --check --strict`, `scripts/upstream_core_slices.sh --strict` |
| WASM interpreter embedding | Not currently exposed through `r-embed` or `r-uniffi`; those crates depend on the native/Android interpreter session and UniFFI runtime surface. WASM support is the pure Rust math/headless-graphics surface. | `rport-flq8`, `rport-mczh` | `scripts/wasm_toolchain_check.sh` |
| Native/compiled package loading | Intentionally rejected for Android-style embedding until a host-specific native extension policy is implemented. Pure-R packages are the release surface. | `rport-ku08`, `rport-rmbf` | `scripts/pure_r_package_corpus.sh --check` |
| Exact `.Random.seed` byte-stream parity | Claimed for the seeded default path: `set.seed`/`RNGkind`/`.Random.seed` and the Mersenne-Twister default reproduce stock R 4.6.1 streams bit-for-bit (`set.seed(42); runif(1)` and the 626-integer `.Random.seed` match stock). Non-default kinds are engine-faithful ports but not individually gated against stock streams. | `rport-pcqa` | Manual A/B probes against stock R 4.6.1 plus conformance RNG fixtures |
| Host UI, network, and native devices | Policy constrained outside the mobile/headless release surface. Unsupported devices or host calls should fail cleanly instead of silently pretending to work. | `rport-a5w7`, `rport-h0jm` | Platform/global scans plus targeted conformance fixtures |

## RNG Policy

RNG state is session-owned, reproducible within a session, and isolated across
parallel sessions. The R-level surface (`set.seed`, `RNGkind`, `runif`,
`rnorm`, `rbinom`, the `r*` sampler family, `sample`) is a port of stock R's
`RNG.c`/`random.c` dispatch: all eight RNG kinds, string-argument validation
with stock error messages, `.Random.seed` round-tripping, and the
Mersenne-Twister default. The nmath distribution samplers draw from the same
session stream via a uniform hook, so seeded sequences match stock R 4.6.1
bit-for-bit on the default kind.

Known divergences from stock R 4.6.1:

- `gc()` reports no cell limits, so the `limit (Mb)` column is always NA and
  the visible table is 2x6 (stock drops that column only when it is all-NA;
  a stock build with a vector-cell limit shows a 2x7 table). Counter values
  are the port's arena statistics, not stock's cell counts.
- RNG warnings render as `Warning message:` blocks without stock's
  `In <call> :` attribution line, matching the port's `warning()` builtin.
- Error attribution for `set.seed`/`RNGkind` validation renders as
  `Error: <message>` (stock prefixes `Error in <call> :`); the message text
  itself matches stock.
- `RNGkind("user-supplied")` and `normal.kind = "user-supplied"` error like
  stock with no dynamic user generator registered; `sample(kind = "Rounding")`
  is accepted (and warned about) but `sample()` still uses the rejection
  sampler.
- `gc(verbose/reset/full)` accepts and ignores `reset`; peak-used counters
  are not resettable.

Conformance fixtures avoid asserting exact random streams unless the result is
deterministic by construction, such as zero-weight sampling. This keeps the
parity gate focused on stock-R behavior contracts: shape, type, replacement
rules, probability validation, tail/log flags, and numeric edge handling.
