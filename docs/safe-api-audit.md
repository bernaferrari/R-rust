# Safe API Audit

The Android and app-facing Rust boundary is intentionally safe and owned:

- `r-embed` exposes `RSession`, `EvalOutput`, `RValue`, package metadata,
  cancellation tokens, and PNG plot bytes as Rust-owned values.
- `r-uniffi` exposes UniFFI records/enums/objects only. Kotlin callers never
  receive raw interpreter pointers.
- Raw `SEXP`, `SEXPTYPE`, and C scalar types stay in the `rmath::sexp` core
  compatibility layer where they are needed to keep the port faithful to R's C
  structure.

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
| `rmath::sexp` | Crate-private runtime implementation | Raw `SEXP` and lifetime-bound internal wrappers are inaccessible to downstream crates |

## Remaining Unsafe Work

The app boundary is clean, but the core interpreter still has legitimate raw
and unsafe internals while the C port is being sessionized. Track those through:

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
and lifetime-bound, thread-confined guards. Legacy UNPROTECT operates only on
the legacy stack. Contexts live in `Box<UnsafeCell<RCNTXT>>`; pointer derivation
uses `UnsafeCell::get` after ownership is stored. Scope checkpoints release
internal transient roots by generation without revoking managed guards.

Internal unsafe routines still require root and aliasing discipline. Miri's
current runs permit exposed provenance; they are useful counterexample checks,
not a formal proof of safety or a security boundary for hostile R programs.

The LOESS numerical engine and headless renderer use `#![forbid(unsafe_code)]`.
The R adapters remain inside the crate-private interpreter boundary. Mutable
arena lends reject re-entry; vector header helpers validate their union tag,
including the runtime's vector-backed BCODESXP representation. Evaluator inputs,
builtin argument lists and temporary internal primitives stay rooted during
nested evaluation. Bytecode variable lookup and writes root their live operand
stack before promise or active-binding evaluation can trigger collection.
