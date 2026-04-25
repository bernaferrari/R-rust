# Performance And Memory Report

Performance is tracked with representative embedding workloads, not only tiny
math microbenchmarks. The current probe covers:

- session startup
- scalar evaluator loops
- vector summary work
- pure-R package loading through Android library paths
- Android headless PNG plot rendering
- four independent sessions running in parallel
- arena active-node and retained-byte snapshots
- Android `libr_uniffi.so` release artifact size

Run the quick local regression check:

```bash
scripts/performance_report.sh --quick --check
```

Run the fuller local report:

```bash
scripts/performance_report.sh --check
```

Artifacts are written to:

- `target/performance/performance-summary.md`
- `target/performance/performance-summary.json`
- `target/performance/android-artifact-size.md`
- `target/performance/android-artifact-size.json`

The thresholds are deliberately loose. They are intended to catch accidental
order-of-magnitude regressions while avoiding flaky CI-style failures on normal
developer machines. Treat a threshold failure as a prompt to inspect the report,
not as a calibrated benchmark claim.

`cargo bench -p rmath --bench bench_eval` remains available for lower-level
allocator, ALTREP, distribution, and raw evaluator timing.

## Current Local Snapshot

This snapshot was produced with:

```bash
scripts/performance_report.sh --quick --check
```

| Workload | Category | Iterations | Avg ms | Arena nodes | Arena bytes | Output bytes |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `startup_session` | startup | 10 | 0.253 | 0 | 0 | 0 |
| `eval_scalar_loop` | eval | 10 | 0.012 | 1580 | 93760 | 8 |
| `eval_vector_summary` | eval | 10 | 0.098 | 1735 | 273820 | 24 |
| `package_load_fresh_session` | package | 10 | 0.321 | 1282 | 71897 | 6 |
| `plot_render_png` | plot | 10 | 9.564 | 1822 | 104848 | 32689 |
| `parallel_four_sessions` | parallel | 10 | 0.564 | 5076 | 285216 | 0 |

Android release artifact size:

| Target | Artifact | Size bytes | Threshold bytes |
| --- | --- | ---: | ---: |
| `aarch64-linux-android` | `libr_uniffi.so` | 4279840 | 52428800 |
