# R-rust

A Rust port of GNU R's core runtime — parser, evaluator, object model,
base/stat semantics, and the `nmath` numerical library — aimed at
embeddable, session-isolated R execution (Android first, desktop and
WASM supported).

**Not a C FFI wrapper.** Every semantic surface is translated Rust,
kept source-shaped against the vendored upstream C so that upstream
behavior can be diffed, ported, and verified hunk-by-hunk.

## Status

| Gate | Result |
| --- | --- |
| Conformance parity vs **R trunk 4.7.0-dev** (r79999, built locally) | **607/607 cases, three-way** (stock C output vs checked-in golden vs Rust output) |
| Script-level differential vs stock R | 10/10 |
| Workspace tests | 2357+ green |
| Clippy (`--workspace --all-targets`, warnings denied in CI) | clean |
| WASM32 toolchain check | passes |

The conformance corpus is regenerated from a locally built trunk R
binary, so the parity contract is anchored to **upstream development
itself**, not a release snapshot.

## What works

- **Parser** — full grammar per `gram.y`: precedence tiers (`^`
  right-associative above unary minus, `%special%` tier, non-associative
  comparisons), native pipe `|>` with `_` placeholder and `=>` pipebind,
  hex/octal literals with overflow semantics, subscript grammar
  (empty slots, `[[i,j]]`, tagged `drop=`/`exact=` args), upstream-shaped
  parse errors (`unexpected ')' in "..."`).
- **Evaluator** — closures with faithful `matchArgs_NR` argument matching
  (exact/partial/positional, stock error messages), lazy `&&`/`||`,
  `tryCatch`/`withRestarts`/`withCallingHandlers` including warning
  handlers, `on.exit` with GC-rooted return values, `data.frame` subsetting
  and `[.data.frame` emulation, vector-growth assignment, recursive `[[`,
  `UseMethod`/`NextMethod` S3 dispatch, S4 classes with `setGeneric`/
  `setMethod`/`slot`/`@`, group-generic `Ops`/`Math`/`Summary` semantics.
- **Rscript semantics** — per-expression auto-print with visibility
  propagation (invisible internals table, handler-decided cases),
  `stop()` halting scripts, deferred warning collection, message
  interleaving, `show.error.locations` markers, call-attributed
  errors/warnings via `deparse1s`.
- **RNG** — Mersenne-Twister default (bit-identical streams to stock:
  `set.seed(1); runif(3)` → `0.2655087 0.3721239 0.5728534`), Inversion
  normals, Rejection sampling, BTPE binomials, `binom.kind` encoding,
  `.Random.seed` in the stock 626-word layout, session-owned isolation,
  Walker alias sampling for >200 categories. Marsaglia-MultiCarry and
  Wichmann-Hill remain selectable.
- **nmath** — faithful translations of the distribution families with
  stream-parity golden tests (`rbinom` bit-identical to stock under the
  same seed; `rbeta` within documented ulp-level libm residuals),
  TOMS 708, Bessel functions (trunk-parity matrix ≤ 1e-15), `fround`/
  `fprec`/`R_pow`/`R_pow_di` with stock fast paths.
- **Base semantics** — dpq vector recycling per `SETUP_MathN`, factor
  group-generic `Ops`/`Math`, `format`/`print` with common-decimal
  encoding, named-vector padding, `sprintf` incl. complex/Inf handling,
  `strptime` ported from `Rstrptime.h` (not libc), `Sys.setenv` via
  libc, `scan(text=)`, `data()` listings, deferred warnings, dump/deparse
  attribute forms.
- **Embedding** — session-owned everything (arena, GC, RNG, paths,
  cancellation), Android/UniFFI facade with owned values, headless
  plot rendering through the portable device registry.

## Layout

```
rmath-rs/
  nmath/            standalone libRmath.a-shaped crate (dist, special,
                    dpq, rng facade, MathState) — no SEXP dependencies
  rmath/            the runtime crate
    src/eval/       parser, evaluator, bytecode, closure matching,
                    dispatch
    src/sexp/       object model, arena GC (persistent-root marks,
                    remembered set, card-free write barriers), protect
    src/mainutils/  builtins: essentials/ (16 domain modules + name
                    registry), subset/subassign, seq, errors, print,
                    serialize, io, rstrptime, ...
    src/library/    stats, methods, graphics, grDevices, grid, utils
  rmath-test/       cross-crate numerical parity suites
crates/
  r-embed/          safe embedding facade (owned values, no raw SEXP)
  r-uniffi/         Kotlin/Swift bindings surface
  r-graphics-engine/  portable plotting primitives
  r-device-android-headless/  PNG device backend
tests/conformance/  607 stock-golden cases + differential runner
scripts/            parity, slices, corpus, release gates
r-source/           vendored upstream C (not part of the crate build)
```

## Build and verify

```bash
cargo build --workspace
cargo test --workspace                 # includes stream-parity goldens
cargo clippy --workspace --all-targets # zero warnings expected

# Conformance vs a local R (release or trunk build on PATH):
PATH="/path/to/R/bin:$PATH" ./scripts/conformance_parity.sh --check
PATH="/path/to/R/bin:$PATH" ./scripts/conformance_parity.sh --regen-goldens
./scripts/wasm_toolchain_check.sh
```

## Upstream sync workflow

1. `git -C r-source fetch origin trunk` and count the delta.
2. `git -C r-source diff HEAD origin/trunk -- src/main src/nmath` per
   file → per-file patches under `plans/upstream-sync-<date>/`.
3. Port each behavioral hunk into the Rust mirror (cosmetic C churn —
   const-cleanup, Makefiles, copyright years — is dispositioned, not
   ported).
4. Regenerate goldens from a trunk build; the three-way parity run
   proves Rust ≡ trunk.

The last sync landed 273 upstream commits (warning-condition classes,
`binom.kind`, `dim2total`, strptime fixes, deparse attribute forms,
`fprec`/`R_pow` alignment — full disposition in
`plans/upstream-sync-2026-08/`).

## Known gaps

Honest ledger, each scoped with a reproduction:

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
- ALTREP wrapper classes, `serialize` format versions beyond the
  implemented surface, and `memory.profile` column fidelity are
  known-gap rows in `docs/upstream-port-map.tsv`.
- `sprintf` on language objects, `str()`/`format()` of calls, and
  condition-object printing are implemented; exotic corner formats
  (`%OS<n>` on some locales) may lag trunk.

## Docs

- `docs/conformance.md` — corpus, normalization, regeneration
- `docs/upstream-port-map.tsv` — C file ↔ Rust module map with sync mode
- `docs/rust-r-port-architecture.md` — crate architecture
- `docs/android-security-policy.md`, `docs/release-gate.md` — embedding
  constraints and release process

## License

GPL-2.0-or-later, matching upstream R. The vendored `r-source/` tree
retains the R Core Team copyright and is used solely as the
diff-and-verify reference.
