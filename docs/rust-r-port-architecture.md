# Rust R Port Architecture

This document describes the current pure-Rust runtime. For the design
rationale and the migration that produced it, see
[`pure-rust-foundation-plan.md`](pure-rust-foundation-plan.md). The
conformance oracle is a pinned GNU R; preserving R language semantics
against it is the project's first constraint.

## Workspace DAG

The runtime is a one-way crate DAG, verified cycle-free:

```
r-math  <  r-core  <  r-stats  ┐
                  <  r-graphics ┴<  r-base  <  r-runtime  <  adapters
```

| Crate | Contents |
| --- | --- |
| `r-math` | Statistical functions exactly as netlib/GNU organize them: distributions, special functions, samplers. Explicit `&mut MathState` and RNG closures — no ambient state. |
| `r-core` | The object model and machine: `ValueId`, `Heap`, `RObject`, interner, GC/roots, parser, evaluator, environments, the primitive-authoring API (`prim`), the primitive-id vocabulary, attribute machinery, print/regex/civil/rbg1 support. |
| `r-stats` | Statistical builtins: dpq surface, samplers, sampling algorithms, summaries, RNGkind/set.seed, loess/predict. |
| `r-graphics` | Graphics builtins: plots, axes, colors, devices, the `par()` surface. |
| `r-base` | The primitive registry (central dispatch), datasets and baked assets, session bootstrap, R-level namespace population (`base.R`, `utils.R`). |
| `r-runtime` | Host integration: session facade, owned-value snapshots, the `embed` boundary module adapters consume. |
| adapters | `r-host-cli`, `r-wasm`, `r-uniffi` — thin platform shells over `r-runtime::embed`. |

Supporting infrastructure: `r-graphics-engine` (low-level scene/render
types), `r-device-android-headless`, `r-device-vello-gpu` (device backends).

Every computational crate carries `#![forbid(unsafe_code)]`; there is no
`unsafe` in the runtime.

## Object model

- **`ValueId`** is an 8-byte `Copy` handle: `(slot, generation)` into a
  direct O(1) generational **`Heap<Slot>`**. There is no `SEXP`, no slab
  cache, no raw pointer projection anywhere.
- **`RObject`** is the typed payload enum: vectors own Rust `Vec`s,
  pairlists and language nodes are struct variants, environments and
  closures are data. GC traces these edges directly.
- **Copy-on-write** discipline: mutation rebuilds object/attribute
  chains rather than mutating aliased objects (`copy_with_attrib` in
  `r-core::attrib` is the canonical shape).
- **GC** is generational mark-and-sweep over explicit root tables owned
  by the heap — no manual protection ledger, no thread-local stacks.

## Ownership and control flow

- All state is explicit in signatures: `&mut Heap`, `&mut SymbolInterner`,
  `&mut RngState`/`&mut MathState`. No `thread_local!` ambient runtime
  state remains.
- Errors are **`Result<ValueId, RSignal>`** end to end — conditions,
  jumps, and returns are data in `RSignal`. There is no panic channel,
  so `panic = "abort"` targets (wasm, Android) are sound.
- Sessions are independent `Heap` instances: isolation is structural,
  verified by the wasm smoke's multi-session oracles.

## The primitive registry and domain split

`r-base` owns the central dispatch (`call_primitive`): builtin ids are
pure constants in `r-core::primitive_ids`, and implementations live in
the domain crates — `r-stats` and `r-graphics` depend only on `r-core`
(and `r-math`/`r-graphics-engine`), never on the registry that consumes
them. The shared authoring layer (`eval_args`, coercion and extraction
helpers) is `r-core::prim`.

Much of GNU's base/utils/stats surface is R-level code, not C builtins:
the bootstrap evaluates pinned per-package R sources
(`assets/rlevel/base.R`, `utils.R`) into their namespace environments,
so closures capture the namespace exactly as GNU's lazy-load installs
them.

## Validation gates

- **Conformance sweep**: 495/495 golden-output cases against the pinned
  GNU oracle, zero mismatches.
- **`rmath-test`**: 18/18 including bit-exact golden RNG streams for
  both walker sampling algorithms.
- **`r-math`**: 103/103 distribution and special-function oracles.
- **uniffi** 33/33, **wasm smoke**: owned values, errors, numeric
  contracts, graphics, and session isolation under `panic = "abort"`.
- Test accounting across the split: r-core 33 + r-base 3 + r-runtime 90
  = the pre-split total exactly.

## Lint policy

Clippy correctness lints are enabled workspace-wide; the unsafe-op
allowlist is retired because the operations it tracked no longer exist.
The remaining style-lint tail from the mechanical C-port translation is
tracked in `rport-c2v5f.16`.
