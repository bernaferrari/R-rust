# Changelog

## 0.1.0 - Unreleased

This is the first Android-focused Rust R runtime slice.

### Supported Highlights

- Session-owned R runtime with isolated arenas, environments, RNG state,
  protect/preserve stacks, output capture, path policy, resource limits, and
  cancellation.
- Rust embedding API through `r-embed` with owned `RValue`, `EvalOutput`,
  package metadata, runtime info, arena stats, and PNG plot bytes.
- UniFFI API through `r-uniffi` for Android/Kotlin hosts.
- Android Compose sample with two independent sessions, eval, package loading,
  S3 package showcase, plot rendering, and cancellation.
- Pure-R package discovery/loading from Android app-private library paths.
- Stock C R conformance harness currently passing 206/206 checked cases.
- Release gates for formatting, strict clippy, focused Rust tests, Android
  aarch64 checks, global-state audit, conformance parity, safe API audit,
  upstream source-map validation, Android packaging, and performance snapshots.

### Known Limits

- This is not a full GNU R replacement yet; the conformance suite covers a
  credible but bounded subset.
- Native package loading through `useDynLib()` is intentionally rejected on
  Android until a host-owned native-library policy exists.
- Graphics support is a small headless plotting subset, not a complete R
  graphics device.
- Exact byte-for-byte `.Random.seed` stream parity is not currently claimed.
- Some translated internals remain C-shaped to preserve upstream reviewability;
  public embedding and Android APIs are Rust-shaped and owned.
