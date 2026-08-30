# Release Gate

`scripts/release_gate.sh` is the local release-candidate proof command. It
collects the checks that matter for the Android-first Rust port into one
repeatable gate with subsystem labels in the output.

The GitHub workflows carry the always-on slice of the same policy split:
`.github/workflows/ci.yml` runs formatting, strict clippy, workspace tests,
stock-R conformance parity, and the WASM build surface as five independent
jobs on the toolchain pinned in `rust-toolchain.toml`; the conformance job
uses r-lib release R, where engine-version-sensitive cases (the R 4.7
`.Random.seed` layout) are expected skips rather than failures.
`.github/workflows/nightly.yml` adds the deeper sweeps: Miri over the sexp
safe-layer test subset and a GC-torture differential stress run. Everything
else below — Android checks, packaging, showcase artifacts, and the safe API
audit — remains the local release-gate bar.


## Commands

Default release gate:

```bash
scripts/release_gate.sh
```

Full local gate, including the slower packaging and generated binding checks:

```bash
scripts/release_gate.sh --full
```

Android packaging without the rest of `--full`:

```bash
scripts/release_gate.sh --android-package
```

Host-only development run when an Android SDK/NDK is not available:

```bash
scripts/release_gate.sh --no-android
```

Do not use `--no-android` for release signoff. The default gate includes the
Android mutable-global scanner and an `aarch64-linux-android` cargo check.

Focused local run without the WASM build surface:

```bash
scripts/release_gate.sh --no-wasm
```

Do not use `--no-wasm` for release signoff. The default gate includes the
`wasm32-unknown-unknown` cargo check for the pure Rust WASM crates.

## Matrix

| Area | Default | Full | Command |
| --- | --- | --- | --- |
| Rust formatting | yes | yes | `cargo fmt --check --all` |
| Rust tests | yes | yes | `cargo test -p rmath`, `cargo test -p r-embed -p r-uniffi` |
| Android global-state scan | yes | yes | `scripts/check_android_globals.sh` through `scripts/android_toolchain_check.sh` |
| Android aarch64 cargo check | yes | yes | `scripts/android_toolchain_check.sh` |
| WASM cargo check | yes | yes | `scripts/wasm_toolchain_check.sh` |
| Android shared library size | yes | yes | `scripts/android_artifact_size.sh --check` |
| Conformance parity | yes | yes | `scripts/conformance_parity.sh --check --report target/release-gate/conformance` |
| Upstream core slices | yes | yes | `scripts/upstream_core_slices.sh --report target/release-gate/upstream-core-slices` |
| Artifact sanity | yes | yes | JSON/Markdown conformance report validation |
| Android showcase artifacts | yes | yes | `scripts/android_showcase_artifacts.sh --check` |
| Public safe API audit | yes | yes | `scripts/audit_safe_api.sh` |
| Upstream port map | yes | yes | `scripts/check_upstream_port_map.sh` |
| Git whitespace | yes | yes | `git diff --check` |
| Desktop host smoke | optional | yes | `scripts/desktop_host_smoke.sh` |
| UniFFI binding generation | optional | yes | `scripts/generate_uniffi_bindings.sh --check` |
| Android Gradle package smoke | optional | yes | `scripts/android_package_smoke.sh --check` |
| Performance report | optional | optional | `scripts/performance_report.sh --quick --check` |
| Strict clippy | yes | yes | `cargo clippy --all-targets --all-features -- -D warnings` |

## Prerequisites

- Rust stable toolchain with `cargo`, `rustfmt`, and the Android/WASM targets
- Stock C R with `Rscript` for the conformance harness
- Python 3 for conformance artifact validation
- Android SDK/NDK for the default Android target check
- Java 17 and Gradle wrapper support for `--full` Android packaging
- `uniffi-bindgen` for `--full`; the binding script installs the pinned CLI if
  it is missing

## Warning Policy

The gate enforces formatting and strict clippy. For compile/test commands it
adds `-Awarnings` to `RUSTFLAGS`, matching the current porting state where some
legacy translated modules still emit ordinary compiler warnings. Do not treat
that as permission to add warnings in Rust-shaped public surfaces; app-facing
crates are also checked by `scripts/audit_safe_api.sh`.

## Artifacts

The gate writes conformance reports to:

- `target/release-gate/conformance/summary.json`
- `target/release-gate/conformance/summary.md`
- `target/release-gate/upstream-core-slices/summary.md`
- `target/release-gate/android-artifacts/android-artifact-size.json`
- `target/release-gate/android-artifacts/android-artifact-size.md`
- `target/release-gate/android-showcase/showcase-transcript.txt`
- `target/release-gate/android-showcase/line-plot.png`
- `target/release-gate/android-showcase/point-plot.png`

The JSON report is checked for nonzero total cases and zero failing or
unexpected-passing cases. The Markdown report is the human-readable release
attachment.

The upstream source map is checked from:

- `docs/upstream-port-map.tsv`

See `docs/upstream-port-map.md` for sync-mode definitions and the comparison
workflow.

The Android showcase script can also be run directly:

```bash
scripts/android_showcase_artifacts.sh --check
```

The Android shared library size gate can also be run directly:

```bash
scripts/android_artifact_size.sh --check
```

The WASM build-surface gate can also be run directly:

```bash
scripts/wasm_toolchain_check.sh
```

Performance and memory report artifacts are separate from the default release
gate because wall-clock timings are machine-sensitive:

```bash
scripts/performance_report.sh --quick --check
```

Release packaging is also explicit, because it writes a distributable local
bundle under `target/release-artifacts`:

```bash
scripts/package_release_artifacts.sh --check
```
