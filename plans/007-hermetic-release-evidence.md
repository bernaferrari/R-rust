# Plan 007: Make CI and release evidence reproducible and truthful

> **Executor instructions**: This plan changes the quality contract. Complete
> it in phases, run each verification, and stop rather than weakening a gate.
> A reviewer maintains the index.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- .github scripts docs .gitignore Cargo.lock rust-toolchain.toml rstudio-mobile`

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: MED
- **Depends on**: Plans 001–006
- **Category**: tests / dx / release
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.3`

## Why this matters

The documented local release gate is much stronger than required CI, but even
it depends on an ignored, unpinned GNU R tree and an interactive-stdin-sensitive
test. Cargo/Yarn/Gradle inputs are not coherently locked, stale UniFFI bindings
pass their “check,” and release bundles do not verify their own checksums or
binding presence correctly. A green build therefore cannot support a 9/10
quality claim or reliably reproduce an artifact from a clean clone.

## Current state

- `scripts/release_gate.sh:172-254` defines formatting, strict Clippy, native,
  Android, Wasm, conformance, performance, package, showcase, API, map, and
  optional full gates.
- `.github/workflows/ci.yml:35-60` runs only a subset; the overlapping
  `parity-gate.yml:139-141` even calls conformance without `--strict`.
- `.gitignore:20` ignores `r-source/`; no submodule/manifest pins it, while
  `check_upstream_port_map.sh:43-52` requires its paths to exist.
- `.gitignore:3` ignores every `Cargo.lock`; `rstudio-mobile/.gitignore:3`
  ignores Kotlin/Yarn resolution. Cargo commands omit `--locked`.
- Android Gradle invokes `cargo ndk`, but CI installs no pinned `cargo-ndk`.
- `generate_uniffi_bindings.sh:13-16,77-82` generates into a temporary
  directory in check mode and exits without diffing checked-in Kotlin output.
- `package_release_artifacts.sh:129-132` uses `find ... >/dev/null`, which
  succeeds with no match, and lines 103-127 never verify recorded hashes.
- `unix::sys_std::tests::test_std_read_console_empty` reads real stdin; the
  full local test run was confirmed blocked in `stdin.read_line` under a PTY.
- The published 577 domain subtotal in `docs/conformance.md` is 575, and no
  gate prevents fixture/domain counts from shrinking.

## Commands

| Purpose | Command | Expected |
|---|---|---|
| Clean bootstrap | run documented bootstrap in a fresh disposable clone | exit 0; no machine-local inputs |
| Canonical gate | `scripts/release_gate.sh --full </dev/null` | exit 0 |
| KMP tests | `cd rstudio-mobile && ./gradlew :shared:allTests :app:testDebugUnitTest :app:lintDebug :webApp:wasmJsBrowserProductionWebpack --no-daemon` | exit 0 |
| UniFFI freshness | perturb a temp copy of checked binding and run `scripts/generate_uniffi_bindings.sh --check` | exits nonzero |
| Release integrity | `scripts/package_release_artifacts.sh --check`, then verify manifest hashes | exit 0; all files/hash checks pass |
| Workflow lint | repository-standard action/YAML lint, if present | exit 0 |

## Scope

**In scope**:

- `.github/workflows/*.yml`, `.github/dependabot.yml`
- Release/build/check scripts under `scripts/`
- `docs/release-gate.md`, `docs/release-packaging.md`, `docs/conformance.md`,
  `NOTICE.md`, licensing metadata after the owner chooses the intended license
- Root ignore/toolchain/lock/bootstrap files and Gradle dependency
  locking/verification metadata
- The single stdin-dependent console test and its narrow injection seam
- A pinned upstream GNU R acquisition manifest/submodule and port-map provenance

**Out of scope**:

- Fixing new semantic failures exposed by the gate; record them as explicit
  xfail/debt, never hide or delete cases.
- Publishing, signing with real credentials, or pushing remote changes.
- Resolving the MIT-vs-GPL product licensing choice without owner/counsel input.

## Git workflow

- Branch `advisor/007-hermetic-release`, isolated worktree.
- Use logical conventional commits (`fix(ci): ...`, `build: ...`, `docs: ...`).
- Never push or modify the user's branch.

## Steps

### Step 1: Pin all source and toolchain inputs

Commit one canonical root Cargo lock and enforce locked/frozen Cargo use. Pin a
Rust toolchain/MSRV, JDK/Gradle/Android SDK+NDK/cargo-ndk, WebR/Yarn resolutions,
and Gradle dependency locks/checksum verification. Pin GitHub actions to full
immutable SHAs with update comments. Add Gradle/npm update automation.

Pin GNU R by submodule or deterministic bootstrap manifest containing URL,
commit SHA, and checksum. Make bootstrap and CI fetch/verify it; include that
SHA in conformance and release provenance.

**Verify**: a disposable clean clone bootstraps without a preexisting
`r-source`, cached dependency graph, or globally installed cargo-ndk.

### Step 2: Make leaf checks authoritative and noninteractive

Fix the stdin test by injecting/simulating EOF rather than reading ambient
stdin. Convert/execute the existing `rmath-test` and standalone differential
binaries as strict gate stages. Make the Web gate production, and run shared,
app unit, lint, browser, and emulator smoke tests at appropriate CI tiers.

Create one reusable required workflow whose named jobs call the same leaf
scripts/strict flags as the local gate. Remove duplicate work from current
workflows, add concurrency cancellation/path filters, and upload reports.

**Verify**: local `--full </dev/null` and the reusable CI workflow enumerate the
same policy matrix and succeed in a clean environment.

### Step 3: Make baselines non-shrinkable

Commit stable conformance case IDs/domain labels and enforce total/domain floors
(at least the current 577), 15 upstream slices, package corpus, and expected map
coverage. Generate status/docs tables from artifacts. Any removal/reclassification
must require an explicit reviewed baseline update.

**Verify**: deleting one fixture, slice, or map row in a temporary diff makes
the corresponding gate fail.

### Step 4: Make binding and release checks real

Generate UniFFI bindings deterministically into temp and byte-diff the exact
checked tree. Make Android compilation depend on freshness and add a packaged
ABI invocation smoke.

Fix binding-presence detection, verify every checksum, use `SOURCE_DATE_EPOCH`,
sort inputs, include complete tracked license/notices, SBOM, and provenance.
Keep signature generation pluggable but do not require private keys locally.

**Verify**: stale binding, missing binding, changed bundle file, and checksum
tampering each fail isolated negative tests; two builds with the same epoch are
byte-identical.

### Step 5: Align documentation and licensing metadata

Generate release/conformance status from gate outputs. Correct the statement
that the tracked repository includes GNU R source unless it now does. Stop for
owner input on MIT vs GPL terms, then align root/crate/app metadata and packaged
license texts to that decision.

## Done criteria

- [ ] Fresh-clone bootstrap uses pinned, locked, verified inputs.
- [ ] One required reusable CI policy mirrors the local full gate.
- [ ] Native differential, KMP unit/lint, production Web, FFI freshness, and
  Android smoke paths are required and upload evidence.
- [ ] Conformance/map baselines cannot shrink silently and docs are generated.
- [ ] Release bundles are deterministic and verify content, hashes, licenses,
  SBOM, and provenance.
- [ ] Full closed-stdin gate passes; negative gate tests fail as intended.

## STOP conditions

Stop if the intended redistribution license is unresolved, an input cannot be
pinned/verified, clean CI requires an undeclared secret, a new failing semantic
case would need deletion/weakening to go green, or verification fails twice.

## Maintenance notes

The local gate and CI should compose the same leaf scripts, not duplicate their
logic. Every new compatibility claim needs a stable case universe and artifact,
not a hand-maintained percentage.

