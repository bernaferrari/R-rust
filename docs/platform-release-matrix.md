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

The Android app now has a checked-in Gradle wrapper, a real `:app:assembleDebug` build, and an optional on-device launch path with `adb install` plus `adb shell am start`.

## Acceptance Commands

Run these commands before closing the platform beads:

```bash
scripts/desktop_host_smoke.sh
scripts/check_android_globals.sh
scripts/android_toolchain_check.sh
scripts/generate_uniffi_bindings.sh --check
scripts/android_package_smoke.sh --check
```

For a connected emulator or device:

```bash
scripts/android_package_smoke.sh --device
```
