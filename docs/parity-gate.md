# Parity Gate

This document mirrors `.github/workflows/parity-gate.yml` and gives the local
commands that reproduce each CI job.

## Prerequisites

- Rust stable toolchain
- `cargo`
- Java 17 for Android checks
- Android SDK for `scripts/android_toolchain_check.sh` and
  `scripts/android_package_smoke.sh`
- `Rscript` (stock C R) for conformance checks
- macOS or Windows if you want to run the desktop host smoke step locally

## Job Equivalents

| CI job | Local equivalent |
| --- | --- |
| Build + test | `cargo fmt --check --all`<br>`cargo clippy --all-targets --all-features -- -D warnings`<br>`cargo build --all-targets`<br>`cargo test --all-targets` |
| Desktop host smoke | `scripts/desktop_host_smoke.sh` |
| Android baseline + tooling | `scripts/android_toolchain_check.sh`<br>`scripts/android_package_smoke.sh --check` |
| UniFFI binding check | `scripts/generate_uniffi_bindings.sh --check` |
| Conformance harness | `cargo build -p rmath`<br>`scripts/conformance_parity.sh --check` |

## Suggested Local Order

Run the build gate first, then the platform-specific checks:

```bash
cargo fmt --check --all
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-targets
cargo test --all-targets
scripts/desktop_host_smoke.sh
scripts/android_toolchain_check.sh
scripts/android_package_smoke.sh --check
scripts/generate_uniffi_bindings.sh --check
cargo build -p rmath
scripts/conformance_parity.sh --check
```
