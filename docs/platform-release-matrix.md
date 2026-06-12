# Platform Release Matrix

This repository tracks the release gaps with executable checks instead of prose-only status.

## Desktop Host

| Tracker | Coverage | Command |
| --- | --- | --- |
| `rport-899` | macOS host parity | `scripts/desktop_host_smoke.sh` on `macos-latest` |
| `rport-z03` | Windows host parity | `scripts/desktop_host_smoke.sh` on `windows-latest` |
| `rport-wxy` | Desktop conformance matrix and CI smoke runs | `.github/workflows/desktop-host-smoke.yml` |

The smoke script launches the desktop CLI, verifies the banner, and confirms a clean shutdown from `q()`.

## Android Baseline

| Tracker | Coverage | Command |
| --- | --- | --- |
| `rport-6mk` | Cross-compile baseline/toolchain | `scripts/android_toolchain_check.sh` |
| `rport-1pc` | JNI/binding generation baseline | `scripts/generate_uniffi_bindings.sh --check` |
| `rport-a0q` | Packaging and smoke validation | `scripts/android_package_smoke.sh` |
| `rport-dg0.2` | Platform global allowlist ratchet | `scripts/check_android_globals.sh` |
| `rport-47vq` | UniFFI Compose showcase and reproducible demo artifacts | `scripts/android_showcase_artifacts.sh --check` |
| `rport-325s` | Upstream C-to-Rust source traceability | `scripts/check_upstream_port_map.sh` |
| `rport-82aj` | Versioned local release artifact bundle | `scripts/package_release_artifacts.sh --check` |
| `rport-mczh`, `rport-flq8` | WASM pure-Rust build surface | `scripts/wasm_toolchain_check.sh` |

The Android app now has a checked-in Gradle wrapper, a real `:app:assembleDebug` build, and an optional on-device launch path with `adb install` plus `adb shell am start`.

### Graphics (Android + WASM)
- Legacy non-portable devices (X11, Quartz, Windows, Cairo, related quartz/win bitmaps) are cfg-gated out on `target_os="android"` (and x11 module). See rport-56d6, rport-tc2t, rport-mfj9.
- Internal R graphics (grDevices::plot etc) on Android uses pure-Rust `DeviceRegistry` + pixel canvas in `rmath library/grdevices/device_registry.rs` (headless, no native).
- Embed `RSession::render(code, w, h)` (and UniFFI) uses portable `r-device-android-headless` (tiny-skia + fontdue -> PNG bytes); works for simple plots, labels, colors, lwd/cex. Font load degrades gracefully (no text) on WASM/no-FS.
- `scripts/wasm_toolchain_check.sh` checks the supported pure Rust WASM surface: `rmath`, `r-graphics-engine`, and `r-device-android-headless` on `wasm32-unknown-unknown` with warnings denied.
- On WASM, rmath core intentionally excludes `library`/`mainutils`/`modules`; the internal `wasm_shim` exists only to back math state. `r-embed` and `r-uniffi` remain native/Android host surfaces until the interpreter session and UniFFI async runtime have target-specific bindings.
- devpictex already gated; devps (PDF/PS) kept as portable. Stubs error cleanly for unsupported snapshot/events etc.

## Acceptance Commands

Run these commands before closing the platform beads:

```bash
scripts/desktop_host_smoke.sh
scripts/check_android_globals.sh
scripts/android_toolchain_check.sh
scripts/wasm_toolchain_check.sh
scripts/generate_uniffi_bindings.sh --check
scripts/android_package_smoke.sh --check
scripts/android_showcase_artifacts.sh --check
scripts/check_upstream_port_map.sh
scripts/package_release_artifacts.sh --check
```

For a connected emulator or device:

```bash
scripts/android_package_smoke.sh --device
```
