# Release Gate

`scripts/release_gate.sh` is the local release-candidate proof command. It
collects the checks that matter for the Android-first Rust port into one
repeatable gate with subsystem labels in the output.

The GitHub workflows use the same policy split: formatting, strict clippy,
focused tests, Android checks, conformance parity, artifact validation, and the
safe API audit are the default release bar. The gate also checks the upstream
port map so C-to-Rust traceability does not decay as modules are rewritten.

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

## Matrix

| Area | Default | Full | Command |
| --- | --- | --- | --- |
| Rust formatting | yes | yes | `cargo fmt --check --all` |
| Rust tests | yes | yes | `cargo test -p rmath`, `cargo test -p r-embed -p r-uniffi` |
| Android global-state scan | yes | yes | `scripts/check_android_globals.sh` through `scripts/android_toolchain_check.sh` |
| Android aarch64 cargo check | yes | yes | `scripts/android_toolchain_check.sh` |
| Conformance parity | yes | yes | `scripts/conformance_parity.sh --check --report target/release-gate/conformance` |
| Artifact sanity | yes | yes | JSON/Markdown conformance report validation |
| Public safe API audit | yes | yes | `scripts/audit_safe_api.sh` |
| Upstream port map | yes | yes | `scripts/check_upstream_port_map.sh` |
| Git whitespace | yes | yes | `git diff --check` |
| Desktop host smoke | optional | yes | `scripts/desktop_host_smoke.sh` |
| UniFFI binding generation | optional | yes | `scripts/generate_uniffi_bindings.sh --check` |
| Android Gradle package smoke | optional | yes | `scripts/android_package_smoke.sh --check` |
| Strict clippy | yes | yes | `cargo clippy --all-targets --all-features -- -D warnings` |

## Prerequisites

- Rust stable toolchain with `cargo`, `rustfmt`, and the Android target
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

The JSON report is checked for nonzero total cases and zero failing or
unexpected-passing cases. The Markdown report is the human-readable release
attachment.

The upstream source map is checked from:

- `docs/upstream-port-map.tsv`

See `docs/upstream-port-map.md` for sync-mode definitions and the comparison
workflow.

The Android showcase script writes separate demo artifacts:

- `target/android-showcase/showcase-transcript.txt`
- `target/android-showcase/line-plot.png`
- `target/android-showcase/point-plot.png`

Generate them with:

```bash
scripts/android_showcase_artifacts.sh --check
```
