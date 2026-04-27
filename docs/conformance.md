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
`r-source/tests/arith.R`, `arith-true.R`, `eval-etc.R`, and `conditions.R`.
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

## Current Status

As of the latest local run:

| Metric | Count |
| --- | ---: |
| Total parity cases | 211 |
| Passing | 211 |
| Failing | 0 |
| Expected failures | 0 |
| Unexpected passes | 0 |

Current domain coverage:

| Domain | Passing Cases | Notes |
| --- | ---: | --- |
| Parser and scalar basics | 34 | Arithmetic, scalar values, comments, infix continuation, early object smoke cases |
| Evaluator, closures, and control flow | 10 | Closures, lexical scope, lazy/default args, missing args, loops |
| Vectors, lists, attributes, and objects | 31 | Vectors, lists, names, subsetting, factors, class replacement, arithmetic attributes |
| Base functions, conditions, and platform helpers | 72 | Sorting/set helpers, output capture, conditions, `ls`, `system`, `proc.time`, file/temp helpers |
| Stats, math, and RNG | 58 | Numeric summaries, distributions, tail/log flags, numeric edge predicates, arithmetic edge cases, `sample`/`sample.int` invariants |
| Packages, namespaces, and S3 | 0 | Covered by unit/package smoke tests today; parity fixtures are tracked by `rport-ifek` and `rport-x3pp` |
| Graphics and Android embedding | 0 | Covered by renderer/unit tests today; parity fixtures are tracked by `rport-c6ap` and `rport-89pz` |
| Error semantics | 6 | Missing argument, `stop`, `stopifnot`, sampling errors, and selected expected errors |

The generated report is the source of truth for exact current counts. Do not
hand-edit release numbers without rerunning the report command.

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
