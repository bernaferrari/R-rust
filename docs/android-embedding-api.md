# Android Embedding API Boundary

The Android-facing API is intentionally an owned-value boundary.

- `rmath::android::RSession` owns a per-session runtime instance.
- `r_embed::RSession` is the public embedding wrapper used by desktop and UniFFI.
- No public Android or embedding method returns raw `SEXP`.
- `r_embed::RSession::eval_result()` returns display output plus an owned `RValue`.
- UniFFI `RSession::eval_result()` returns the same boundary shape as an
  `EvalResult { output, value }` record for Kotlin/Java callers.
- Legacy `r_embed::RSession::eval()` remains as a string-output convenience wrapper.
- Long-running evaluations can opt into cooperative cancellation with
  `r_embed::CancellationToken`; the token is explicit and per evaluation.

`SEXP` remains inside the runtime core. Code that crosses into Android, Kotlin,
or other FFI callers should convert immediately into `RValue`, `String`,
`Vec<u8>`, or another owned Rust type.

Current `RValue` coverage includes null, logical, integer, real, string vectors,
lists, unsupported values with display text, and errors. It is deliberately
lossless enough for app inspection without requiring callers to parse printed R
output.

Mutable runtime state should be reached through an active `RSession`. Shared
process state is limited to immutable sentinels/caches and documented platform
fallbacks.
