# Android Runtime Security Policy

The Android embedding is app-owned, session-owned, and pure Rust at the public
boundary. Kotlin and UniFFI callers interact with `RSession`, owned `RValue`
results, cancellation tokens, and PNG bytes; raw `SEXP` values remain inside the
runtime core.

## Files And Paths

- Android hosts configure paths with `configure_android_paths(app_files_dir,
  cache_dir, bundled_library_dir)`.
- The writable package library is always derived as
  `app_files_dir/R/library`.
- `tempdir()` and `tempfile()` use `cache_dir/Rtmp`.
- A bundled package library may be added as a read-only search root after the
  writable user library.
- The configured paths are stored on `RInstance`, so separate sessions can use
  different app directories without process-global path state.
- `.libPaths()`, `find.package()`, `library()`, `require()`, `tempdir()`, and
  `tempfile()` resolve through the active session policy.

## Package Loading

- The Android runtime supports pure-R packages from the configured library
  roots.
- Namespace imports, export patterns, S3 registrations, and repeated
  `library()`/`require()` calls are session-local.
- Native package loading is intentionally rejected. A `useDynLib()` directive
  returns a clear error because Android app package loading needs an explicit
  host-owned native-library policy, not implicit `dlopen` behavior.
- Direct native entrypoints are rejected too: `.Call()`, `.C()`, `.Fortran()`,
  `.External()`, `dyn.load()`, `dyn.unload()`, and `library.dynam()` report
  policy errors instead of silently returning `NULL` or attempting process-wide
  native loading.

## Processes And Shell

- On Android, `system()` is disabled by runtime policy and reports an R error.
- On host platforms, `system()` keeps stock-R-like behavior for conformance
  tests and development parity.
- Forking and Unix process helpers are outside the Android embedding surface.

## Network

- The current policy does not grant special network privileges. Any network
  access available to R code is limited by the Android app manifest and the
  platform socket APIs.
- Package installation from remote repositories is not part of the shippable
  Android surface yet; packages should be bundled or copied into the app-private
  library root.

## Cancellation And Resources

- Long-running evaluation uses cooperative cancellation through explicit tokens.
  Cancellation is scoped to the evaluation call and restored afterward.
- Android hosts can set per-session limits for evaluation depth, cooperative
  wall-clock checks, arena bytes, and arena node count.
- Cancellation and time checks cover evaluator loops and common Android-facing
  entry points. Native blocking calls should be avoided or wrapped in future
  host-owned policies before they become Android-facing.
- Preemptive CPU quotas are outside the current pure-R runtime and should be
  enforced by the Android host process/thread policy if arbitrary untrusted code
  is accepted.

## Release Gates

- `scripts/android_toolchain_check.sh` cross-checks the Android target and
  mutable-global allowlist.
- `scripts/check_android_globals.sh` rejects new Android-facing mutable process
  globals unless they are intentionally documented.
- `scripts/release_gate.sh` runs formatting, clippy, tests, Android checks,
  conformance parity, safe API audit, and whitespace checks.
