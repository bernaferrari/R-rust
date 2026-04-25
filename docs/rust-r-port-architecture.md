# Rust R Port Architecture

This port keeps R's semantics and source shape where that helps conformance,
but the ownership model is Rust-first. C-shaped entrypoints are compatibility
shells; Rust-facing code should use session-owned state, typed `Sexp` handles,
and explicit embedding boundaries.

## Layers

1. **Raw compatibility layer**

   `SEXP`, `SEXPTYPE`, `Rf_*`, `.Internal` shims, and translated routines live
   here. This layer may stay close to upstream R so future C changes can be
   replayed and compared. Raw pointers should not cross into Android, UniFFI, or
   new Rust APIs.

2. **Instance and memory layer**

   `RInstance` owns mutable interpreter state: arena, environments, protect
   stack, preserve stack, RNG state, caches, output capture, path policy,
   graphics state, and evaluator control state. `RArena` owns allocation and
   `Sexp<'a>` is an owner-scoped handle proving that a raw pointer is either
   owned by the active session/arena or is an immutable sentinel.

3. **Rust runtime layer**

   New runtime code should prefer APIs such as `RSession::sexp`,
   `RSession::eval_sexp`, `RSession::eval_sexp_in`, `EvalContext`, and
   `eval_expr`. `Rf_eval` remains for ported internals that still speak raw
   `SEXP`, but it should delegate inward rather than owning evaluator policy.

4. **Embedding layer**

   `rmath::android`, `r_embed`, and `r_uniffi` return owned Rust values:
   `RValue`, `String`, `Vec<u8>`, and records. They must not expose `SEXP` or
   depend on mutable process-global state. Android hosts configure app-private
   paths explicitly and run each session on its owning worker thread.

## Ownership Rules

- Every mutable R runtime operation needs an active `RSession`/`RInstance`.
- No new mutable process global should be added for Android-facing behavior.
- Raw `SEXP` wrapping belongs at owner boundaries: `RArena::sexp` or
  `RSession::sexp`.
- Public Rust APIs should accept and return `Sexp<'_>` or owned values, not raw
  pointers.
- Raw C ABI compatibility functions should be thin shells around typed Rust
  helpers.
- Session state should be movable into `RInstance` before a feature becomes
  Android-facing.
- Cross-thread hosts should use one `RSession` per worker/thread. Sharing a live
  session across workers is outside the current safety contract.

## Evaluator Shape

The evaluator is Rust-shaped at its primary boundary:

- `EvalContext<'a>` binds an owner-scoped environment.
- `eval_expr(expr, env)` owns cancellation and visibility setup.
- `RSession::eval_sexp*` proves pointer ownership before evaluation.
- `Rf_eval` converts raw pointers and delegates to `eval_expr`.

This keeps the port faithful inside the evaluator while avoiding a C-shaped API
for new Rust code.

## Android Policy

Android embedding is app-owned and session-owned:

- `configure_android_paths(app_files_dir, cache_dir, bundled_library_dir)` sets
  the user library, cache/temp directory, and bundled package library for one
  session.
- `.libPaths()`, `find.package()`, `library()`, `require()`, `tempdir()`, and
  `tempfile()` resolve through `RInstance::path_policy`.
- `render(code, width, height)` returns PNG bytes from a headless renderer and
  evaluates plot data on the worker session.
- `system()` is disabled on Android. Host builds keep it enabled for stock-R
  parity and conformance checks.
- Native package loading through `useDynLib()` is rejected until an Android
  host-owned native-library policy exists.
- Mutable-global additions must pass `scripts/check_android_globals.sh`.

## Upstream Sync Workflow

1. Import or diff the target upstream R C file under `r-source`.
2. Identify the behavioral unit: parser, evaluator, builtin, math routine,
   device, package helper, or platform shim.
3. Keep translated control flow close to upstream inside raw compatibility
   modules when that improves reviewability.
4. Move ownership, allocation, state, cancellation, paths, and output into
   `RInstance`/`RSession` instead of preserving process globals.
5. Add or update the typed Rust entrypoint first, then make the C-shaped shim
   delegate to it.
6. Add focused Rust tests for the typed API and Android/embedding tests when the
   behavior crosses FFI boundaries.
7. Add conformance cases when behavior is user-visible R semantics.
8. Run the parity and Android gates before committing.

## Conformance Gates

Run these for changes that affect evaluator, SEXP ownership, Android, or
embedding behavior:

```bash
RUSTFLAGS=-Awarnings cargo check -p rmath -p r-embed -p r-uniffi
RUSTFLAGS=-Awarnings cargo test -p rmath --lib -- --test-threads=1
RUSTFLAGS=-Awarnings cargo test -p r-embed -p r-uniffi -- --test-threads=1
RUSTFLAGS=-Awarnings cargo test -p rmath --doc
scripts/conformance_parity.sh
RUSTFLAGS=-Awarnings cargo check --target aarch64-linux-android -p rmath -p r-embed -p r-uniffi
scripts/check_android_globals.sh
git diff --check
```

For narrow non-embedding changes, run the subset that covers the touched layer,
but do not skip `scripts/conformance_parity.sh` when R-visible behavior changes.
