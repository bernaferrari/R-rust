# R-rust

An experimental, from-scratch Rust translation of GNU R's core runtime —
parser, evaluator, SEXP object model with arena GC, base/stat semantics
slices, the `nmath` numerical library, and portable graphics — aimed at
embeddable, session-isolated R execution (Android first, desktop hosts
second). Repository: <https://github.com/bernaferrari/R-rust>.

It is **not a C FFI wrapper**: every semantic surface is translated Rust,
kept source-shaped against the vendored upstream C so upstream behavior
can be diffed, ported, and verified hunk-by-hunk.

## What it is

- A Rust runtime library (`rmath`) implementing a curated subset of R:
  the full parser grammar, a tree-walking evaluator with S3/S4 dispatch,
  a SEXP object model with a generational arena GC, and translated
  slices of base, stats, methods, graphics, grDevices, grid, and utils.
- `rmath-nmath`, a standalone, SEXP-free crate for the numerical
  distribution/special-function code (dist, dpq, special, RNG facade).
- Embedding surfaces: `r-embed` (owned-value facade), `r-uniffi`
  (Kotlin/Swift bindings), an Android headless PNG device, and a desktop
  CLI/REPL host (`r-host-cli`).
- A verification rig: a 608-case curated behavioral fixture corpus run
  three-way (stock C output vs checked-in golden vs Rust output),
  a script-level differential harness, and stream-parity RNG goldens.

## What it is NOT

- **Not drop-in R.** It will not run arbitrary CRAN packages or
  real-world R scripts unchanged. The `.Internal`/builtin surface is a
  curated subset, not the full R API.
- **Not a native package runtime.** No C-extension loading, no CRAN
  binary compatibility, no `compiler`/bytecode evaluation path.
- **Not a full WASM R.** The WASM32 target covers the math and graphics
  core only (nmath + plotting primitives), not the whole evaluator.
- **Not a safe port, and not a sandbox.** The safe embedding facade is
  experimental; nothing here has been audited as a memory-safety or
  isolation boundary. See [Safety status](#safety-status).
- **Not at 100% fidelity.** Compatibility is claimed only where the
  curated corpus pins it; [Known gaps](#known-gaps) lists the rest.

## Architecture

```
rmath-rs/
  nmath/            standalone libRmath.a-shaped crate (dist, special,
                    dpq, rng facade, MathState) — no SEXP dependencies
  rmath/            the runtime crate
    src/eval/       parser, evaluator, bytecode surface, closure matching,
                    dispatch
    src/sexp/       object model, arena GC (persistent-root marks,
                    remembered set, write barriers), protect
    src/mainutils/  builtins: essentials/ (16 domain modules + name
                    registry), subset/subassign, seq, errors, print,
                    serialize, io, rstrptime, ...
    src/library/    stats, methods, graphics, grDevices, grid, utils
  rmath-test/       cross-crate numerical parity suites
crates/
  r-embed/          embedding facade (owned values; experimental)
  r-uniffi/         Kotlin/Swift bindings surface
  r-graphics-engine/  portable plotting primitives
  r-device-android-headless/  PNG device backend
tests/conformance/  608 curated fixtures + three-way differential runner
tests/script-diff/  script-level differential vs stock R
tests/differential/ standalone numeric d/p/q harness vs fixed R 4.x
                    reference values (own crate, excluded from the
                    workspace; `cargo run --manifest-path
                    tests/differential/Cargo.toml`)
scripts/            parity, slices, corpus, release gates
r-source/           vendored upstream C reference (fetched, not built,
                    not committed; see scripts/fetch-r-source.sh)
```

Design details: `docs/rust-r-port-architecture.md`. The C-file ↔ Rust
module map with per-file sync mode lives in
`docs/upstream-port-map.tsv`.

## Safety status

**Experimental. Do not rely on it for anything that matters yet.**

- The object model is being reworked (copy semantics, external-pointer
  representation). Interfaces and invariants are moving; treat every
  release as disposable.
- The `r-embed` "safe" facade is an *API design goal* (owned values, no
  raw SEXP in user code), not an audited guarantee.
- No memory-safety audit, no fuzzing at the boundary, no isolation
  claims: this is not a security boundary for running untrusted R code.
- Known soundness work is tracked openly; e.g. ALTREP is disabled
  pending an external-pointer redesign (unsound representation).

## Compatibility status

All parity claims are **against the exact oracle described below**, on a
**curated subset** of R behavior — not against R's own test suites:

- **608/608 curated behavioral fixtures pass, three-way** (stock C
  output vs checked-in golden vs Rust output). The corpus is a curated
  subset selected to pin ported upstream hunks; it is *not* "608 of R's
  tests", and passing it does not imply general conformance.
- Script-level differential vs stock R: 10/10 curated scripts.
- Workspace tests: 2357+ green. Clippy (`--workspace --all-targets`,
  warnings denied in CI): clean. WASM32 toolchain check: passes.
- Default RNG: Mersenne-Twister with bit-identical streams to stock
  (`set.seed(1); runif(3)` → `0.2655087 0.3721239 0.5728534`).

Covered areas include: full parser grammar with upstream-shaped parse
errors; closures with `matchArgs_NR` argument matching; `tryCatch`/
`withRestarts`/`withCallingHandlers`; S3 `UseMethod`/`NextMethod` and
S4 classes; Rscript auto-print/visibility semantics; MT-stream-parity
RNG with stock `.Random.seed` layout; translated nmath distribution
families with stream-parity goldens (TOMS 708, Bessel matrix ≤ 1e-15);
`data.frame` subsetting; `strptime` ported from `Rstrptime.h`.

## Supported platforms

- **Android** (first-class): aarch64 / armv7 / i686 / x86_64 via
  cargo-ndk; headless PNG rendering through the portable device
  registry; UniFFI Kotlin bindings.
- **Desktop hosts** (macOS arm64 is the development platform; Linux
  supported): `r-host-cli` REPL, differential/conformance harnesses.
- **WASM32**: math and graphics core only (see [What it is NOT](#what-it-is-not)).
- Windows is not exercised by CI; no claims are made for it.

## Exact R oracle

The parity contract is anchored to **upstream development itself**, not
a release snapshot:

- **Oracle binary:** R trunk **r90451** ("Unsuffered Consequences",
  2026-08-27), built locally, from wch/r-source commit
  `bac583951b728e97b9786804d3b4081f0fe18df5`.
- **Machine-readable pin:** `oracle/r-oracle.json` records that full commit,
  the commit archive URL and SHA-256, and the expected runtime identity. CI
  validates the manifest, hash-verifies and builds that exact source, then
  rejects any `Rscript` without the matching provenance marker. Moving
  `release`/`devel` comparisons run separately at night and are informational;
  they can never satisfy the required exact-oracle gate.
- **Vendored reference tree:** pinned at the last sync base,
  `d4cc5d9e196a144bbb087a798bb945b37121383b` — exactly 273 commits
  behind the oracle commit. Reproduce it with:

  ```bash
  ./scripts/fetch-r-source.sh   # clone + hash-verify the pinned commit
  ```

  The script refuses to continue from any other commit and prints the
  R version of the checked-out tree. Goldens are regenerated only from
  the oracle above (`scripts/conformance_parity.sh --regen-goldens`).

## Upstream provenance and sync

1. `git -C r-source fetch origin trunk` and count the delta.
2. `git -C r-source diff HEAD origin/trunk -- src/main src/nmath` per
   file → per-file patches under `plans/upstream-sync-<date>/`.
3. Port each behavioral hunk into the Rust mirror (cosmetic C churn —
   const-cleanup, Makefiles, copyright years — is dispositioned, not
   ported).
4. Regenerate goldens from the trunk oracle; the three-way parity run
   proves Rust ≡ trunk on the curated corpus.

The last sync landed **273 upstream commits** (warning-condition
classes, `binom.kind`, `dim2total`, strptime fixes, deparse attribute
forms, `fprec`/`R_pow` alignment — full disposition in
`plans/upstream-sync-2026-08/`).

## Build and verify

The toolchain is pinned by `rust-toolchain.toml` (currently Rust
1.96.0) and dependencies are locked in `Cargo.lock`.

```bash
cargo build --workspace
cargo test --workspace                 # includes stream-parity goldens
cargo clippy --workspace --all-targets # zero warnings expected

# Conformance vs the trunk oracle (R binary on PATH):
./scripts/install_r_oracle.sh             # prints the pinned R bin directory
PATH="/path/to/R/bin:$PATH" ./scripts/conformance_parity.sh --check
PATH="/path/to/R/bin:$PATH" ./scripts/conformance_parity.sh --regen-goldens
./scripts/wasm_toolchain_check.sh
```

`r-embed` selects exactly one dense linear-algebra backend. Its default is the
portable pure-Rust/faer implementation. Desktop hosts that provide compatible
Fortran BLAS/LAPACK can select the system profile explicitly:

```bash
cargo check -p r-embed --no-default-features --features fortran-backend
```

The `rust-backend` and `fortran-backend` features are mutually exclusive;
building with both or neither is a compile-time error. The system profile is
not supported on Android or WASM.

## Known gaps

Honest ledger, each scoped with a reproduction:

- **ALTREP disabled pending external-pointer redesign (unsound
  representation)**; remaining wrapper-class work is tracked in
  `docs/upstream-port-map.tsv`.
- Mersenne-Twister is the only *stream-parity* engine; the alternative
  kinds run but their streams are not bit-verified against stock.
- A few samplers (`rbeta`, `rnorm` under some shapes) differ from stock
  by ≤ 3 ulps from libm `exp`/`log` rounding — consumption order is
  identical, verified against a standalone compiled C reference.
- Trunk's own decimal parser (`R_strtod5`) is inexact; this port parses
  decimals correctly-rounded instead (documented choice; identical for
  ≤ 16 significant digits).
- `srcref`-level error locations are not implemented;
  `show.error.locations` works at the expression level.
- `serialize` format versions beyond the implemented surface and
  `memory.profile` column fidelity are known-gap rows in
  `docs/upstream-port-map.tsv`.
- `sprintf` on language objects, `str()`/`format()` of calls, and
  condition-object printing are implemented; exotic corner formats
  (`%OS<n>` on some locales) may lag trunk.
- No compiler/bytecode evaluation path; namespaces/imports beyond the
  implemented surface; no locale-complete runtime.

## Roadmap

Direction, not promises — ordered by current work:

1. **Object model rework**: Sexp copy/move semantics and the
   external-pointer redesign; re-enable ALTREP on the new
   representation.
2. **Broaden the corpus** beyond the curated 608 fixtures toward
   sampled real-world R scripts, keeping the three-way methodology.
3. **Stream-parity for alternative RNG kinds** (Marsaglia-MultiCarry,
   Wichmann-Hill) or explicit stream-difference documentation per kind.
4. **Compiler/bytecode evaluation path** study (upstream `compiler`
   package semantics).
5. **Audited safe embedding API v1** (`r-embed`), with fuzzing at the
   boundary and a stated security policy for untrusted code.
6. **WASM evaluator scope decision**: either grow beyond the math +
   graphics core or document the core-only boundary permanently.

## License and provenance

GPL-2.0-or-later, matching upstream R. The full text is in
[COPYING](COPYING); [LICENSE](LICENSE) summarizes licensing and
provenance. The vendored `r-source/` tree retains the R Core Team /
R Foundation copyright, is used solely as the diff-and-verify
reference, is not part of the crate build, and is reproducible from the
pinned commit via `scripts/fetch-r-source.sh`.
