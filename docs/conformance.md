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
exports/imports/S3 directives, `packageVersion()`/`packageDescription()`,
namespace-only loading, namespace-qualified access, source-form package data,
explicit `envir` loading, source-form `LazyData`, serialized data policy
errors, same-name package isolation across sessions, and explicit native-code
or bytecode package rejection. The release gate runs this script after the
stock-R performance comparison.

## Current Status

As of the latest local run:

| Metric | Count |
| --- | ---: |
| Total parity cases | 407 |
| Passing | 407 |
| Failing | 0 |
| Expected failures | 0 |
| Unexpected passes | 0 |

Current domain coverage:

| Domain | Passing Cases | Notes |
| --- | ---: | --- |
| Parser and scalar basics | 121 | Arithmetic, scalar values, comments, infix continuation, parse/deparse/dput/bquote/RDS/unname/expression/mode/storage-mode/tsp/comment-attribute/attr/attributes/dim/length-replacement, shape-helper, array-creation, and `drop()` dimension-reduction cases, `substitute()` promise lookup, named-argument IO smoke including readLines/writeLines, locale/capability/runtime-introspection/platform/date/umask/Sys.info shape smoke, zero-length paste/bind/read.fwf shape cases, early object smoke cases |
| Evaluator, closures, and control flow | 10 | Closures, lexical scope, lazy/default args, missing args, loops |
| Vectors, lists, attributes, and objects | 53 | Vectors, typed vector constructors including legacy `single()` marker attributes, lists, names, subsetting, factors, class replacement/unclass/oldClass behavior, date/time class attributes, data-frame helper transforms, directory and file listing shape cases, row/col matrix-shape helpers, arithmetic attributes |
| Base functions, conditions, and platform helpers | 134 | Sorting/set helpers, output capture, conditions, `ls`, `system`/`system2`, `Sys.getenv`/`Sys.setenv`, `Sys.getpid`, `gcinfo()`/`gctorture()` session-state behavior, reflective builtin absence checks, `pi` and version constant binding semantics, non-GNU helper-alias absence semantics, options/getOption including defaults, broader `sprintf` integer/float formats and recycling errors, `proc.time`, file/temp helpers including `file.info`, `file.size`, `file.mtime`, `cat(file=...)`, path-string `readLines`, `file.access`/append/copy/link/symlink/rename/remove, `normalizePath`, `Sys.readlink`, unique per-session `tempfile()`, wrapping helpers, language-call `deparse()` parity, character transform NA/zero-length parity, message/warning concatenation, match.arg/char.expand edge behavior, zero-length `format.data.frame`, and factor `levels<-` remapping |
| Stats, math, and RNG | 58 | Numeric summaries, distributions, tail/log flags, numeric edge predicates, arithmetic edge cases, complex hyperbolics, `sample`/`sample.int` invariants |
| Packages, namespaces, and S3 | 4 | Package namespace and S3 fixtures plus `system.file()` and the pure-R package corpus gate |
| Graphics and Android embedding | 1 | Base graphics layout state parity plus renderer/unit tests; broader graphics parity remains tracked by `rport-pluy` |
| Error semantics | 26 | Missing argument, `stop`, `stopifnot`, sampling errors, matrix-helper validation, dmultinom validation, match/char-expand/sprintf/read.fwf/gcinfo/gctorture2/function-lookup expected errors, and selected expected errors |

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

## Release Gaps

The parity suite is strong enough to catch regressions in the covered surface,
but it is not a full R compatibility claim. The release roadmap tracks the
remaining coverage expansions:

- `rport-az2r`: base language parity top-50 gaps
- `rport-x3pp`: S3 release parity beyond the first registry slice
- `rport-dgn7`: stats, RNG, and numeric fidelity release slice
- `rport-ifek`: pure-R package corpus smoke test
- `rport-c6ap`: graphics path release quality
- `rport-89pz`: Android UniFFI release surface hardening

New behavior should land with a stock-R fixture whenever possible. If exact
stock-R parity is intentionally out of scope for Android, add a policy note and
an owner bead instead of silently broadening claims.

## RNG Policy

RNG state is session-owned, reproducible within a session, and isolated across
parallel sessions. Conformance fixtures avoid asserting exact random streams
unless the result is deterministic by construction, such as zero-weight
sampling. This keeps the parity gate focused on stock-R behavior contracts:
shape, type, replacement rules, probability validation, tail/log flags, and
numeric edge handling. Exact byte-for-byte `.Random.seed` stream parity with a
specific stock R release is a separate compatibility target and should be
tracked explicitly if required by a package or demo.
