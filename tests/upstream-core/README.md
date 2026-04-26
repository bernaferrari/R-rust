# Upstream Core Slices

These fixtures are curated from GNU R's upstream `r-source/tests/*.R` files.
They keep the upstream intent and comments, but avoid surfaces the embedded
runtime intentionally does not ship yet, such as full recommended packages,
graphics devices, source-reference round trips, and host-specific timing output.

Run them with:

```bash
scripts/upstream_core_slices.sh --report target/upstream-core-slices
```

The harness compares stock C R (`Rscript --vanilla`) with the Rust runtime using
the same runner as `tests/conformance`. Known unsupported upstream expectations
belong in `xfail.tsv` with an owner bead; passing slices should stay xfail-free.
