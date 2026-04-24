# Conformance Matrix

`scripts/conformance_parity.sh` runs a small golden suite against two engines:

- stock C R via `Rscript`
- the Rust interpreter runner in `tests/conformance`

The harness:

- reads scripted cases from `tests/conformance/cases/*.R`
- normalizes output deterministically
- compares both engines against checked-in goldens in `tests/conformance/golden/*.out`
- treats entries in `tests/conformance/xfail.tsv` as known gaps that must have
  an owner bead and reason
- exits successfully with a clear skip message if `Rscript` is not available

## Usage

```bash
./scripts/conformance_parity.sh
```

The script is non-interactive and safe to run in CI. It uses `Rscript --vanilla`
and a standalone Rust runner, so it does not depend on the interactive REPL.

## Current cases

- `001_arithmetic.R` checks scalar arithmetic
- `002_min_scalar.R` checks scalar builtin evaluation
- `003_integer_vector.R` checks integer vector formatting
- `004_logical_scalar.R` checks logical comparison output
- `005_string_scalar.R` checks string formatting
- `006_control_flow.R` checks `if` expression evaluation
- `007_scalar_math.R` checks math builtin composition
- `008_na_arithmetic.R` checks scalar `NA` arithmetic behavior
- `009_index_assignment.R` checks basic index assignment behavior
- `010_factor_labels.R` checks factor label formatting

## Domain Matrix

| Domain | Status | Gate | Owner |
| --- | --- | --- | --- |
| Parser syntax | Seeded | `scripts/conformance_parity.sh` cases 001, 006 | `rport-ur1` |
| Evaluator/scoping/promises | Early | Android unit eval tests plus scalar parity cases | `rport-t57` |
| Vector semantics | Seeded | parity cases 003, 008; xfail case 009 | `rport-c2w` |
| Base functions | Early | parity case 002 plus Android eval smoke tests | `rport-6p2` |
| Stats/math | Seeded | parity case 007, Android dnorm/pnorm/Bessel tests | `rport-pm0` |
| Packages/namespaces | Open | no parity gate yet | `rport-97s` |
| Object systems | Open | xfail case 010; no S3/S4 gate yet | `rport-fs5` |
| Graphics/grid/grDevices | Infrastructure | session-state unit tests and Android global ratchet | `rport-5jd` |
| Android embedding API | Seeded | `scripts/android_toolchain_check.sh`, `android::tests` | `rport-usi` |
| Android cancellation | Open | no gate yet | `rport-ece` |
| Android package paths | Open | no gate yet | `rport-k0l` |
| Platform globals/session safety | Gated | `scripts/check_android_globals.sh` and Android target compile | `rport-dg0.2` |

## Gap Report

The project is past proof-of-concept for isolated sessions, scalar/vector smoke
evaluation, math wrappers, and Android cross-compilation. It is not yet close to
full R compatibility: parser coverage is narrow, promises and lexical scoping
need stock-R goldens, package loading is mostly policy work, and graphics still
needs the Android device bridge.

Current parity status is 8 passing cases and 2 expected failures:

| Case | Owner | Gap |
| --- | --- | --- |
| `009_index_assignment.R` | `rport-c2w` | subassignment is not implemented |
| `010_factor_labels.R` | `rport-fs5` | factor construction/printing is incomplete |

Near-term conformance work should land in this order:

1. Expand parser/evaluator goldens for functions, calls, promises, lexical
   scope, assignment forms, and errors.
2. Expand vector goldens for recycling, attributes, coercion, subsetting,
   factors, lists, complex/raw vectors, and NA/NaN edge cases.
3. Add base function goldens for `match`, `unique`, `order`, `sort`, strings,
   dates, connections, conditions, and serialization.
4. Add stats/math goldens with tolerances for distributions, RNG, linear
   algebra, FFT, optimization, and model helpers.
5. Add package/namespace fixtures for a minimal pure-R package, then decide the
   Android native package policy.
6. Add graphics goldens once the Android device bridge can capture deterministic
   plot artifacts.

## Skip behavior

If `Rscript` is missing, the harness prints a deterministic skip message and
returns exit code `0`. That keeps local and CI runs from failing unexpectedly
when stock C R is not installed.
