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
| `rmath::sexp` | Core runtime and compatibility API | Raw `SEXP` exists here by design; owner-checked wrappers such as `Sexp<'a>` are the Rust-shaped path |

## Remaining Unsafe Work

The app boundary is clean, but the core interpreter still has legitimate raw
and unsafe internals while the C port is being sessionized. Track those through:

- `rport-0dbg`: strict clippy and Rust 2024 unsafe-op cleanup
- `rport-x3pp`: object/S3 parity, including package-created list-object S3 dispatch
- `rport-e6q`: older broad unsafe/raw SEXP audit issue; use this document and
  `rport-erop` as the release-facing audit source

The standard for future app-facing additions is simple: return owned values or
lifetime-bound wrappers, and do not expose raw pointers through `r-embed` or
`r-uniffi`.
