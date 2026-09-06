# Boundary fuzzing

Coverage-guided harnesses for the r-embed boundary invariant: arbitrary
input to `RSession::eval` / `define_handle` / guards must never escape as
a Rust panic — only `RSessionError` crosses.

- `eval_boundary`: structured R-ish scripts (token grammar in `src/lib.rs`)
  fed to a long-lived thread-local session.
- `handle_boundary`: full handle lifecycle per input, ending in a stale
  read that must error.

`crates/r-embed/tests/boundary_stress.rs` checks the same invariant
deterministically (fixed seed) on every CI run; the initial version of
that stress test found three real escaping-panic bugs (top-level
`break`/`next`, top-level `return(v)`, empty-script rooting).

## Running

```
cargo +nightly fuzz run eval_boundary -- -max_total_time=300 -rss_limit_mb=16384 -malloc_limit_mb=8192
cargo +nightly fuzz run handle_boundary -- -max_total_time=300 -rss_limit_mb=16384 -malloc_limit_mb=8192
```

Baseline runs (2026-09, M2 Max): eval_boundary 13,250 execs / 4,375 cov /
531 corpus / 0 crashes in 241s; handle_boundary 10,100 execs / 6,928 cov /
622 corpus / 0 crashes in 241s.

Notes:

- Keep the `-rss_limit_mb` / `-malloc_limit_mb` overrides: session init
  reserves gigabytes and libFuzzer's defaults (2 GB) trip on the
  reservation even though it never becomes RSS.
- When writing `Unstructured`-driven generators: an exhausted
  `Unstructured` keeps yielding default values forever — always bound
  token loops with `while !u.is_empty()`. An unbounded loop here was the
  original "ASan init hang" (unbounded String growth produced the giant
  mallocs), not the engine.

`fuzz/corpus/` is the seed corpus; `fuzz/artifacts/` holds findings
(crash-/oom-/timeout- prefixed files) — commit only deliberate seeds.
