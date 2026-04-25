# Android Embedding API Boundary

The Android-facing API is intentionally an owned-value boundary.

- `rmath::android::RSession` owns a per-session runtime instance.
- `r_embed::RSession` is the public embedding wrapper used by desktop and UniFFI.
- No public Android or embedding method returns raw `SEXP`.
- `r_embed::RSession::eval_result()` returns display output plus an owned `RValue`.
- UniFFI `RSession::eval_result()` returns the same boundary shape as an
  `EvalResult { output, value }` record for Kotlin/Java callers.
- UniFFI sessions expose `is_active()`, `close()`, and
  `cancel_current_operation()` as Android-friendly lifecycle controls. The older
  `destroy()` and `cancel()` names remain as compatibility aliases.
- Android hosts should call `configure_android_paths(appFilesDir, cacheDir,
  bundledLibraryDir)` before evaluation when app-private library and temp paths
  are known. The configured paths drive `.libPaths()`, `find.package()`,
  `library()`, `require()`, `tempdir()`, and `tempfile()` for that session.
- `render(code, width, height)` evaluates simple numeric `plot(...)`
  expressions on the worker session and returns PNG bytes. The current Android
  renderer supports points, lines, combined point/line plots, title and axis
  labels, tick labels, common colors, `lwd`, and `cex`. Width and height must be
  at least 32 pixels.
- Legacy `r_embed::RSession::eval()` remains as a string-output convenience wrapper.
- Long-running evaluations can opt into cooperative cancellation with
  `r_embed::CancellationToken`; the token is explicit and per evaluation.
- Android hosts can set per-session resource limits for evaluation depth,
  cooperative wall-clock time checks, arena bytes, and arena node count.

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

The Android sandbox and package/process policy is documented in
[`android-security-policy.md`](android-security-policy.md).

## Kotlin Shape

```kotlin
val session = RSession()
check(session.is_active())

val paths = android_runtime_paths(
    appFilesDir.absolutePath,
    cacheDir.absolutePath,
    bundledLibraryDir?.absolutePath,
)
session.configure_android_runtime(paths)

val result = session.eval_result("c(1, 2, 3)")
val linePlot = session.render(
    "plot(c(1, 2, 3), c(1, 4, 9), type = \"l\", col = \"blue\", lwd = 2)",
    800u,
    600u,
)
val pointPlot = session.render(
    "plot(c(1, 2, 3), c(3, 1, 2), type = \"p\", col = \"green\", cex = 1.4)",
    800u,
    600u,
)

session.cancel_current_operation()
session.close()
```

Errors are typed through `RError`: invalid package names and zero-sized plots
return `InvalidInput`, closed sessions return `SessionClosed`, cancelled
evaluations return `Cancelled`, and R/runtime failures carry an explanatory
string in `EvalError`, `RenderError`, or `InitFailed`. Plot dimensions below
the renderer minimum return an actionable `RenderError`.
