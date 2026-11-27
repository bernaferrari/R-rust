# Safe API Audit

The workspace is memory-safe by construction, enforced at compile time
rather than audited by inspection: every computational crate carries
`#![forbid(unsafe_code)]` — `r-math`, `r-core`, `r-base`, `r-stats`,
`r-graphics`, and `r-runtime`. There is no `SEXP`, no raw interpreter
pointer type, and no `unsafe` block anywhere in the runtime; the C-ABI
compatibility shims and the unsafe-op allowlist (`docs/unsafe-op-allowlist.tsv`)
were retired with the pure-Rust foundation.

## Crate DAG and boundary shapes

| Layer | Public shape | Safety enforcement |
| --- | --- | --- |
| `r-math` | Pure statistical functions over `&mut MathState` and explicit RNGs | `#![forbid(unsafe_code)]` |
| `r-core` | `ValueId(slot, generation)` + `Heap<Slot>` + `RObject` enum; parser, evaluator, environments | `#![forbid(unsafe_code)]` |
| `r-stats` | Statistical builtins over `r-core`/`r-math` | `#![forbid(unsafe_code)]` |
| `r-graphics` | Graphics builtins over `r-core`/`r-graphics-engine` | `#![forbid(unsafe_code)]` |
| `r-base` | The primitive registry and session bootstrap | `#![forbid(unsafe_code)]` |
| `r-runtime` | Host integration: `r_runtime::embed` re-export boundary, session facade, owned `RValue` snapshots | `#![forbid(unsafe_code)]` |
| `r-uniffi` | UniFFI records, enums, objects over `r-runtime` | Owned values only |
| `r-wasm` | `WasmRSession` over `r-runtime::embed` | Owned values across the JS boundary |
| `r-host-cli` | CLI host over `r-runtime` | Owned values only |

The dependency order is one-way, verified cycle-free:
`r-math < r-core < {r-stats, r-graphics} < r-base < r-runtime < adapters`.

## Ownership model

- Values are 8-byte `Copy` `ValueId(slot, generation)` handles into a
  direct O(1) generational `Heap<Slot>`. Object payloads are the typed
  `RObject` enum — no untyped pointer projections exist.
- All runtime state is explicit: `&mut Heap`, `&mut SymbolInterner`,
  `&mut RngState`/`&mut MathState` thread through signatures. No
  `thread_local!` ambient state remains in the runtime.
- Control flow is `Result<ValueId, RSignal>` end to end; there is no
  panic-based error channel, so `panic = "abort"` targets (wasm,
  Android) are sound.
- GC uses generational mark-and-sweep over explicit root tables; roots
  are values in `Heap`-owned structures, not a manual protection ledger.

## Lint policy

Clippy correctness lints are enabled workspace-wide, including
`never_loop`, `while_immutable_condition`, `mut_range_bound`,
`absurd_extreme_comparisons`, `drop_non_drop`, and `dead_code`. The
remaining crate-level allows in `r-math`/`r-runtime` cover style lints
inherited from the mechanical C-port translation; retiring that tail is
tracked in `rport-c2v5f.16`. Site-specific allows carry justification
comments where a lint pattern is intentional (Fortran range-bound reuse,
regex greediness flags retained for the unimplemented GNU extension
surface, parser bookkeeping fields).

The standard for app-facing additions is unchanged: return owned values
or lifetime-bound wrappers; never expose raw handles through
`r-runtime::embed` or `r-uniffi`. The compile-time forbid makes most of
the historical audit categories impossible rather than merely audited.
