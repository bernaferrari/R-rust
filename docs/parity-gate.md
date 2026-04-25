# Parity Gate

This document mirrors `.github/workflows/parity-gate.yml` and gives the local
commands that reproduce each CI job. For release-candidate signoff, prefer the
single local command in `docs/release-gate.md`:

```bash
scripts/release_gate.sh
```

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
| Format + focused tests | `cargo fmt --check --all`<br>`env RUSTFLAGS="-Awarnings" cargo test -p rmath -- --test-threads=1`<br>`env RUSTFLAGS="-Awarnings" cargo test -p r-embed -p r-uniffi -- --test-threads=1` |
| Desktop host smoke | `scripts/desktop_host_smoke.sh` |
| Android baseline + tooling | `scripts/android_toolchain_check.sh`<br>`scripts/android_package_smoke.sh --check` |
| UniFFI binding check | `scripts/generate_uniffi_bindings.sh --check` |
| Conformance harness | `cargo build -p rmath`<br>`scripts/conformance_parity.sh --check` |

## Suggested Local Order

`scripts/release_gate.sh` is the maintained local order. If you need to debug a
specific CI job manually, run the matching commands directly:

```bash
cargo fmt --check --all
env RUSTFLAGS="-Awarnings" cargo test -p rmath -- --test-threads=1
env RUSTFLAGS="-Awarnings" cargo test -p r-embed -p r-uniffi -- --test-threads=1
scripts/desktop_host_smoke.sh
scripts/android_toolchain_check.sh
scripts/android_package_smoke.sh --check
scripts/generate_uniffi_bindings.sh --check
scripts/conformance_parity.sh --check --report target/conformance-report
```
