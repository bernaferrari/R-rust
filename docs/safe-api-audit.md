# Safe API Audit

The Android and app-facing Rust boundary is intentionally safe and owned:

- `r-embed` exposes `RSession`, `EvalOutput`, `RValue`, package metadata,
  cancellation tokens, and PNG plot bytes as Rust-owned values.
- `r-uniffi` exposes UniFFI records/enums/objects only. Kotlin callers never
  receive raw interpreter pointers.
- Remaining raw `SEXP` projections, `SEXPTYPE`, and C scalar types stay inside
  the crate-private interpreter. Checked node identities and owning handles
  replace raw storage as the core is modernized.

Run the checked audit:

```bash
scripts/audit_safe_api.sh
```

The release gate runs this script by default.

## Current Boundary

| Layer | Public Shape | Unsafe/Raw Policy |
| --- | --- | --- |
| `r-uniffi` | UniFFI records, enums, and `RSession` object | No `unsafe`; no raw `SEXP`; owned values only |
| `r-embed` | Safe Rust `RSession`, `RValue`, package and plot APIs | No `unsafe`; no raw `SEXP`; owned values only |
| `rmath::android` | Rust session facade over the interpreter core | Owned `RValue` surface; internal raw access stays below this layer |
| `rmath::sexp` | Crate-private runtime implementation | Raw `SEXP` and internal checked handles are inaccessible to downstream crates |

## Remaining Unsafe Work

The app boundary is clean, but the core interpreter still contains raw and
unsafe internals. Track the remaining ownership and execution work through:

- `rport-hah9u.32`: remaining raw runtime/context fields
- `rport-sg9a`: explicit runtime borrowing and retirement of ambient TLS access
- `rport-0dbg`: remaining Rust 2024 unsafe-op cleanup below the app boundary
- `rport-x3pp`: object/S3 parity, including package-created list-object S3 dispatch
- `rport-e6q`: older broad unsafe/raw SEXP audit issue; use this document and
  `rport-erop` as the release-facing audit source

The standard for future app-facing additions is simple: return owned values or
lifetime-bound wrappers, and do not expose raw pointers through `r-embed` or
`r-uniffi`.

## Runtime containment and rooting

The translated `sexp`, `eval`, `mainutils`, `library`, `modules`, and platform
runtime modules are crate-private. This intentionally removes the experimental
raw public API: shared SEXP access must not permit ambient evaluation, mutation,
or GC to invalidate an outstanding borrow. External hosts use owned results
and the `r-embed` handle API. Compile-fail fixtures reject raw module access;
these privacy tests are not a separate proof of every internal lifetime rule.

Rust roots use stable `(slot, generation)` identities, owner-aware replacement,
and owning, thread-confined handles and guards. Legacy UNPROTECT operates only on
the legacy stack. Contexts live in `Rc<UnsafeCell<RCNTXT>>`; pointer derivation
uses `UnsafeCell::get` after ownership is stored. Scope checkpoints release
internal transient roots by generation without revoking managed guards.

Options and cached base wrappers now store actual owning `Sexp` values. Queries
snapshot those values before callbacks; eviction or replacement releases their
roots without a permanent preserve ledger. Source and private-bytecode flat
replacement calls retain separate owning syntax and execution graphs, resolve
the setter once, and preserve the original shared RHS. The checked node factory
publishes complete promise expression/environment/value links before collection.

S4 primitive generics, method lists, validity functions, inheritance tables,
and the deferred-default marker now retain their actual values through owning
fields. Cache replacement and clearing release those roots. Dispatch snapshots
the values before calling R; its status guard restores the original physical
runtime even when a callback revokes availability. Task callbacks likewise own
their function and data, snapshot inputs before evaluation, and restore running
and visibility state on their original runtime. Stable callback identities make
removal and insertion during iteration deterministic. The public callback
wrapper's optional-data, naming, indexing, and warning compatibility remains
tracked in `rport-2gpp.1.4`.

`eval()` owns the selected expression and environment through callbacks and
collection. A NULL element in an expression vector participates in evaluation
and visibility rather than being skipped. Return transfers are consumed only
by the matching eval context; unmatched transfers continue to unwind. Direct
native entries use the same original-owner transfer scope as managed calls.
Closure duplication shares FORMALS, BODY, and CLOENV as GNU R does, snapshots
those edges before allocation, and duplicates attributes according to the
requested depth. The collector and object representation are unchanged.

The bounded acceptance evidence and its limits are recorded in
[`runtime-ownership-acceptance.md`](runtime-ownership-acceptance.md). Typed native
dispatch (`rport-wpdk.1`), private host roots (`rport-jxfp.10`), and exact console
capture (`rport-jxfp.11`) remain separate integration obligations; owning runtime
fields do not by themselves establish those contracts.

Internal unsafe routines still require root and aliasing discipline. Targeted
strict-provenance Miri runs with default borrow checking cover binding
publication callbacks, owning options/cache eviction, and replacement graphs
through collection. Some historical runs used exposed provenance. These are
bounded executable checks, not a formal proof of every internal path.

The LOESS numerical engine and headless renderer use `#![forbid(unsafe_code)]`.
The R adapters remain inside the crate-private interpreter boundary. Mutable
arena lends reject re-entry; vector helpers validate their type and payload kind,
including the runtime's vector-backed BCODESXP representation. Evaluator inputs,
builtin argument lists and temporary internal primitives stay rooted during
nested evaluation. Bytecode variable lookup and writes root their live operand
stack before promise or active-binding evaluation can trigger collection.

String/list element access now validates target tags, buffer presence and signed
indices before reading or writing payloads in both debug and release builds. String
setters share the generational write barrier used by list setters. Targeted
Miri reproduced an out-of-range string write before the fix, then passed the
same regression and invalid-tag/index tests afterward. These checks still
require a live initialized SEXP; they cannot make arbitrary raw pointers safe.

The owned LOESS kernel receives a per-operation execution policy, checks a
conservative workspace estimate and polls cancellation within numerical loops.
It returns errors to the adapter and never stores the callback in R models.
