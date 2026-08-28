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

## Upstream Core Slices

`scripts/upstream_core_slices.sh` runs curated excerpts adapted from GNU R's
`r-source/tests/arith.R`, `arith-true.R`, `eval-etc.R`, `conditions.R`,
`any-all.R`, `structure.R`, `complex.R`, `primitives.R`, `eval-fns.R`,
`simple-true.R`, `print-tests.R`, `reg-IO2.R`, and `reg-tests-1b.R`.
Unlike the numbered conformance fixtures, these cases compare live stock
`Rscript --vanilla` output directly against the Rust runtime. They are intended
for evaluator/arithmetic regression work and the release gate:

```bash
scripts/upstream_core_slices.sh --report target/upstream-core-slices
```

Known unsupported upstream expectations belong in
`tests/upstream-core/xfail.tsv` with an owner bead. Passing slices should remain
xfail-free.

If `Rscript` is missing, the harness prints a deterministic skip message and
returns `0`. That keeps local runs from failing unexpectedly on machines without
stock R installed; release gates should install stock R and require this command.

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

## Current Status

As of the latest local run:

| Metric | Count |
| --- | ---: |
| Total parity cases | 583 |
| Passing | 583 |
| Failing | 0 |
| Expected failures | 0 |
| Unexpected passes | 0 |

Current domain coverage:

| Domain | Passing Cases | Notes |
| --- | ---: | --- |
| Parser and scalar basics | 22 | Arithmetic, scalar values, comments, infix continuation, parse/deparse/dput/bquote/RDS/unname/expression/mode/storage-mode/tsp/comment-attribute/attr/attributes/dim/length-replacement, repetition helpers, tabulation, findInterval boundary options including single-break edges, serialize version/ascii/xdr header and payload handling including ASCII string escapes, shape-helper, array-creation, and broad parser/runtime smoke cases |
| Evaluator, closures, and control flow | 25 | Closures, lexical scope, lazy/default args, missing args, loops, and evaluator visibility/control-flow checks |
| Vectors, lists, attributes, and objects | 163 | Vectors, typed vector constructors, lists, names, name-preserving repetition, list/complex repetition, named and ordered mixed-list unlisting, recursive unlist control, typed rle/inverse.rle, raw-vector serialization roundtrips including ASCII hex-byte payloads, subsetting, factors, explicit missing factor levels, generated factors, factor coercion, factor summaries, interval cutting, ordered factors, ordered comparisons, interaction factors, releveling, droplevels, class/attribute replacement, matrices, data frames, S4 slots, and grouped object helpers |
| Base functions, conditions, and platform helpers | 172 | Sorting/set helpers, output capture, conditions, search-path/environment helpers, options, file/temp/path helpers, connections, platform state, `.Internal` dispatch, and non-GNU alias absence semantics |
| Stats, math, and RNG | 124 | Numeric summaries, distributions, beta/F TOMS 708 branches, beta tail/log flags and quantile/CDF roundtrips, arithmetic edge cases, complex hyperbolics, `sample`/`sample.int`, pmin/pmax missing-value and character coercion semantics, typed cumulative extrema, typed cumulative sum/product NA semantics, typed `diff()` integer/logical and overflow semantics, array margin summaries, aggregate/tapply/by grouped summaries, and summary-vector parity |
| Packages, namespaces, and S3 | 10 | Package namespace and S3 fixtures, `system.file()`, S3 method-export absence, and the pure-R package corpus gate |
| Graphics and Android embedding | 4 | Base graphics layout state and external graphics dispatch smoke; broader graphics parity remains tracked separately |
| Error semantics | 63 | Missing argument, `stop`, `stopifnot`, sampling errors, matrix-helper validation, dmultinom validation, match/char-expand/sprintf/read.fwf/gcinfo/gctorture2/function-lookup expected errors, serialization input validation, internal dispatch errors, `relevel()` validation, parse errors rendered like Rscript (`unexpected ')' in "<context>"`, `unexpected end of input`), and selected platform expected errors |

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
| Full GNU R compatibility | Not claimed by the Android release subset. The checked suite and curated upstream slices are the active proof boundary. | `rport-pcqa`, `rport-65tc` | `scripts/conformance_parity.sh --check --strict`, `scripts/upstream_core_slices.sh` |
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
