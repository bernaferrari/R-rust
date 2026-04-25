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

The Android app now has a checked-in Gradle wrapper, a real `:app:assembleDebug` build, and an optional on-device launch path with `adb install` plus `adb shell am start`.

## Acceptance Commands

Run these commands before closing the platform beads:

```bash
scripts/desktop_host_smoke.sh
scripts/check_android_globals.sh
scripts/android_toolchain_check.sh
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
