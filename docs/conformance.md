# Conformance parity checks

`scripts/conformance_parity.sh` runs a small golden suite against two engines:

- stock C R via `Rscript`
- the Rust interpreter runner in `tests/conformance`

The harness:

- reads scripted cases from `tests/conformance/cases/*.R`
- normalizes output deterministically
- compares both engines against checked-in goldens in `tests/conformance/golden/*.out`
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

## Skip behavior

If `Rscript` is missing, the harness prints a deterministic skip message and
returns exit code `0`. That keeps local and CI runs from failing unexpectedly
when stock C R is not installed.
