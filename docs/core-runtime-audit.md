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

`eval.rs` is functional for the covered runtime slice and is now much closer to
the final Rust shape. Primitive metadata, evaluator limits, and application of
closures/specials/builtins live in focused modules; `PRIMPRINT` follows
upstream's `((eval / 100) % 10)` rule; and the old `eval::builtin` placeholder
delegates to the canonical function table. `eval::apply` makes the important R
semantic boundary explicit: unevaluated builtins are dispatched before ordinary
argument evaluation, while evaluated builtins go through one call-frame path.
`eval::builtin` now owns explicit unevaluated and evaluated handler tables, so
`eval::apply` is small again and only coordinates evaluation policy,
visibility, and S3/S4/primitive fallback behavior.
Base environment registration now constructs canonical `R_FunTab` primitives
where the table's argument policy matches the binding, marks noncanonical
Rust-side helpers with `PRIMOFFSET = -1`, and lets `eval::apply` prefer
`PrimitiveDescriptor` names with call-head fallback only for those explicit
noncanonical helpers. The raw compatibility shell in `eval.rs` now denies
`unsafe_op_in_unsafe_fn`, and `eval::builtin` has the same boundary, so the
remaining pointer conversions are explicit unsafe blocks rather than ambient
module-wide unsafety.

Android-facing sessions are now explicitly thread-confined in the type system.
Each `RSession` owns an `RInstance`; the remaining `thread_local!` slot is only
a compatibility bridge that says which instance is active on the current worker
while legacy R-shaped accessors run. Ordinary Android use does not require a
mutable process-global interpreter: create one session per tab/worker, keep it
on that worker, and configure paths, RNG, cancellation, output capture, and
package state through the session. The only atomic in this path is an optional
per-call cancellation token (`Arc<AtomicBool>`) used when another host thread
needs to request cooperative cancellation; it is not shared interpreter state.

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
