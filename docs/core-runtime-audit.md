# Core Runtime Audit

This audit compares the Rust-shaped core against the upstream GNU R C sources
that still matter most for runtime behavior:

- `r-source/src/include/Rinternals.h`
- `r-source/src/main/memory.c`
- `r-source/src/main/eval.c`
- `r-source/src/main/arithmetic.c`

## Current Verdict

`SEXP` ownership is on the right Rust path. `Sexp<'a>` is a typed, lifetime-bound
view; `RArena` owns allocation; and `RSession` rejects foreign arena pointers at
the safe session boundary. Raw `SEXP` still exists inside `rmath::sexp`, but it
is no longer the shape exposed to Android or embedding callers. Primitive SEXP
accessors now live in a focused object submodule, and allocator/GC type handling
matches on `SEXPTYPE` variants rather than raw tag numbers.

`arithmetic.rs` is the cleanest core file. It denies `unsafe_op_in_unsafe_fn`,
uses the typed `NumericVector` boundary, and now delegates divide, modulo, and
floor-division edge behavior to normal IEEE division and the R-shaped
`myfmod`/`myfloor` helpers. The new zero-division conformance fixture checks
stock R parity for `Inf`, `-Inf`, `NaN`, and integer `NA` behavior.

`eval.rs` is functional for the covered runtime slice and is moving toward the
final Rust shape. Primitive metadata and evaluator limits are now separated into
focused modules; `PRIMPRINT` follows upstream's `((eval / 100) % 10)` rule; and
the old `eval::builtin` placeholder delegates to the canonical function table.
The remaining weak spot is the large builtin dispatch match and the broad
`unsafe_op_in_unsafe_fn` allow still needed by that compatibility shell.

## Test Strategy

Run stock GNU R tests, but curate them. The full upstream test suite expects the
entire GNU R distribution, recommended packages, devices, S4/compiler behavior,
and platform-specific output. The useful path is to import upstream slices into
the parity harness, keep explicit xfails for unsupported surfaces, and grow the
passing set until those xfails disappear.

Run C-vs-Rust benchmarks too, but only after output parity for the benchmarked
program. `scripts/compare_stock_r_performance.sh` now does that for small
overlapping programs and writes Markdown/JSON reports under
`target/stock-r-performance`.

## Remaining Beads

The remaining core-runtime work is tracked in beads:

- `rport-9oeg`: import upstream GNU R evaluator and arithmetic test slices
- `rport-ruic`: make evaluator dispatch Rust-shaped and modular
- `rport-1wxz`: finish arithmetic parity edge work
- `rport-i3x6`: shrink SEXP unsafe internals and split object module
- `rport-t7nl`: stress Android parallel sessions without shared mutable runtime state
