# Release Packaging

Release artifacts are local, reproducible bundles. The project is not published
from this repository by default.

## Build A Bundle

From a clean checkout with Rust, Java 17, Android SDK/NDK, and
`uniffi-bindgen` available:

```bash
scripts/release_gate.sh --full
scripts/performance_report.sh --quick --check
scripts/package_release_artifacts.sh --check
```

The package script writes:

```text
target/release-artifacts/rport-<version>/
  README.md
  CHANGELOG.md
  NOTICE.md
  docs/
  bindings/kotlin/
  android/jniLibs/arm64-v8a/libr_uniffi.so
  reports/android-artifact-size.md
  reports/android-artifact-size.json
  manifest.txt
  manifest.sha256
```

## Versioning

The bundle version comes from `workspace.package.version` in the root
`Cargo.toml`. Crates that do not inherit workspace metadata should use the same
version before a release bundle is cut.

## Android Layout

The Android native library is placed at:

```text
android/jniLibs/arm64-v8a/libr_uniffi.so
```

Kotlin bindings are generated into:

```text
bindings/kotlin/
```

This mirrors the Gradle/Android source-set layout used by the Compose sample.

## Checks

`scripts/package_release_artifacts.sh --check` verifies that:

- Kotlin UniFFI bindings were generated.
- The Android shared library exists and is non-empty.
- changelog, notice, README, and core docs are included.
- the manifest and SHA-256 checksum file are non-empty.

Run `scripts/generate_uniffi_bindings.sh --check` independently when changing
the UniFFI API surface.
