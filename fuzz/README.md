# Boundary fuzzing

Coverage-guided harnesses for the r-embed boundary invariant: arbitrary
input to `RSession::eval` / `define_handle` / guards must never escape as
a Rust panic — only `RSessionError` crosses.

- `eval_boundary`: structured R-ish scripts (token grammar in `src/lib.rs`)
  fed to a long-lived session.
- `handle_boundary`: full handle lifecycle per input, ending in a stale
  read that must error.

`crates/r-embed/tests/boundary_stress.rs` checks the same invariant
deterministically (fixed seed) on every CI run.

## Runbook (and the current constraint)

```
cargo +nightly fuzz run eval_boundary -- -max_total_time=300 -rss_limit_mb=16384 -malloc_limit_mb=8192
cargo +nightly fuzz run handle_boundary -- -max_total_time=300 -rss_limit_mb=16384 -malloc_limit_mb=8192
```

Known constraint on this engine: under ASan the interpreter's session
initialization is minutes-slow (the process reserves gigabytes; libFuzzer
with the default `-rss_limit_mb=2048`/`-malloc_limit_mb` OOMs on the very
first `malloc(4294967296)` arena reservation — raise both as above), and
`-s none` currently fails to link the engine's static archive. Until the
init cost is reduced (or a small-arena fuzz profile lands), prefer the
deterministic stress test for routine runs and budget long ASan sessions
for dedicated fuzzing days.

The `fuzz/corpus/` and `fuzz/artifacts/` directories are inputs/outputs,
not source; artifacts committed here would be findings.
