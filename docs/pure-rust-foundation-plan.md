# Pure-Rust Foundation Architecture & Migration Plan

Status: **Approved architectural direction (2026-11-25)**  
Baseline measured at: `667,123 LOC` across `1,729` Rust files (`crates/rmath` = `598,540 LOC`, `6,528` `unsafe` blocks, `1,227` `unsafe fn`s, `19,956` `SEXP` references, `65` `thread_local!` blocks, `189` duplicate `do_*` builtins).

## 1. Strategic Decisions (Non-Negotiable Constraints)

The goal of `rport` is **the cleanest, safest, fastest Rust implementation of the R language**, preserving R's observable language semantics (verified against the pinned GNU R oracle `bac583951b`) across native CLI, Wasm/browser, and mobile.

To eliminate the dual-architecture tax of impersonating GNU R's 1990s C internals, the following compatibility layers are **explicitly dropped**:

1. **Drop C ABI compatibility.** No `SEXP` raw-pointer ABI, no `#[no_mangle]` C runtime exports, no `extern "C"` internal dispatch, no `dlopen`/`rdynload`/foreign `.so` loading, no `PROTECT`/`UNPROTECT` C stack, no `R_alloc`/`vmaxget`/`vmaxset`, no `staticlib`/`cdylib` or `libRmath.a` shim.
2. **Drop GNU R `.rds`/`.RData`/XDR wire format compatibility.** Replace the ~12.6k LOC of XDR/ASCII/native-endian V1–V3 serializers, `saveload.rs`, `xdr.rs`, and multi-codec plumbing with a single compact, safe, versioned Rust binary graph format (backed by `flate2`). Convert bundled `datasets` and `portable_package` assets to this native format once.
3. **Drop the optional `fortran-backend`.** Pure-Rust `faer` is the sole linear algebra backend. Delete `fortran-backend` feature gates, Fortran FFI declarations, `accelerate.rs`, `veclib_g95c.rs`, `init_win.rs`, and `build.rs` system LAPACK linking.

**Preserved invariants:**
- R language behavior and statistical/numerical results match the pinned GNU R oracle (except intentionally changed wire formats for `save`/`load`/`serialize`/`unserialize` and disabled dynamic C `.so` loading).
- Multi-session isolation (`one session per thread`, zero mutable process-global state) remains mandatory for Wasm, Android, iOS, and desktop embedders.
- Every migration milestone keeps the build green and passes conformance, unit, and safety checks before committing directly to `main`.

---

## 2. Target Architecture ("As If C Never Existed")

### 2.1 Object Model & Heap (`ValueId` + Direct Generational Arena)

Today, `sexp/heap.rs` already stores canonical graph edges as safe 16-byte `NodeLink` IDs, yet every `CAR`/`CDR`/`TYPEOF` projects those IDs into raw `SEXP = *mut SexprecCore` pointers and then immediately reverse-looks them up through a thread-local `BTreeMap` (`SLAB_META`) + `SLAB_CACHE` while cloning multiple `Rc`s and copying `SexprecCore` headers.

**Target design (`#![forbid(unsafe_code)]`):**

- **`ValueId`**: a `Copy`, 8-byte generational index (`slot: u32, generation: u32`) — or `u64` bit-packed with reserved sentinel constants (`ValueId::NIL`, `ValueId::MISSING`, `ValueId::UNBOUND`, `ValueId::EMPTY_ENV`, `ValueId::BASE_ENV`, `ValueId::GLOBAL_ENV`).
- **`Heap`**: owns a flat `Vec<Slot>` (with free list + generation bump on reuse) and typed payload storage:
  ```rust
  pub struct Slot {
      generation: u32,
      gc_mark: u8,
      named: u8,       // reference-count / copy-on-modify bits
      flags: ObjectFlags,
      attrib: ValueId, // ValueId::NIL when absent
      body: RObject,
  }

  pub enum RObject {
      Nil,
      Symbol { name: SymId, value: ValueId, internal: ValueId },
      Pairlist { car: ValueId, cdr: ValueId, tag: ValueId },
      Language { car: ValueId, cdr: ValueId, tag: ValueId },
      Closure { formals: ValueId, body: ValueId, env: ValueId },
      Environment { frame: EnvFrame, enclos: ValueId },
      Promise { expr: ValueId, env: ValueId, value: ValueId },
      Primitive(BuiltinId),
      Logical(Vec<i32>),   // or compact bit/byte representation with NA
      Integer(Vec<i32>),
      Real(Vec<f64>),
      Complex(Vec<RComplex>),
      String(Vec<InternedStr>),
      Raw(Vec<u8>),
      List(Vec<ValueId>),
      Expression(Vec<ValueId>),
      Bytecode(Rc<BytecodeChunk>),
      Altrep(Box<dyn AltrepProvider>),
      External(Rc<dyn Any>),
      WeakRef { key: ValueId, value: ValueId, finalizer: ValueId, on_exit: bool },
      S4Object,
  }
  ```
- **O(1) access**: `heap.get(id)` is a single array index + generation check — eliminating `SLAB_META`, `SLAB_CACHE`, `Cell<SexprecCore>`, pinned `Rc` pages, `Sexp<'a>`'s 8-field refcount overhead, and thousands of `unsafe` blocks.
- **Rooting & GC**:
  - One unified root mechanism: `RootStack` (scoped stack frames for evaluator temporaries) + `RootTable` (stable generational handles for host `ValueHandle`s and long-lived runtime caches).
  - Retire `LegacyProtectionStack`, `Rf_protect`/`Rf_unprotect`, and the 4,629 manual `protect()` calls.
  - Non-moving mark-and-sweep (or generational mark-and-sweep) traces `RObject` directly via one exhaustive `match` — no unsafe pointer re-derivation.

### 2.2 Explicit Runtime Ownership (Zero TLS)

Retire all `65` `thread_local!` blocks, `CURRENT_INSTANCE`, `CurrentInstanceGuard`, and the compiler-unchecked P1/P2 raw `*mut RInstance` aliasing rules:

- **`Runtime`** owns disjoint subsystems so Rust's borrow checker enforces aliasing statically:
  - `pub heap: Heap`
  - `pub symbols: SymbolInterner`
  - `pub eval: EvalState` (call stack / `RCNTXT` frames, limits, cancellation, visibility)
  - `pub errors: ErrorState` (warnings, condition handlers, restarts)
  - `pub rng: RngState` and `pub math: MathState` (passed explicitly into `r-math`, removing TLS from `crates/nmath`)
  - `pub env_vars: SessionEnv` (per-session `HashMap<String, String>`, eliminating `static mut environ` and UB-prone `std::env::set_var`)
  - `pub io: IoState` (connections, output capture, virtual FS, path policy)
  - `pub graphics: GraphicsState` (device list + active `r-graphics-engine` target)
- `RenderPlotBackendGuard`'s `transmute`-based lifetime erasure is replaced by a closure-scoped API (`runtime.with_draw_target(target, |rt| ...)`).

### 2.3 Typed Control Flow (`Result<ValueId, RSignal>`)

Replace `std::panic::panic_any(RError)` + `catch_unwind` with ordinary Rust `Result`:

```rust
pub type RResult<T = ValueId> = Result<T, RSignal>;

pub enum RSignal {
    Error(RError),
    Return { target_frame: FrameId, value: ValueId },
    Break { target_loop: LoopId },
    Next { target_loop: LoopId },
    Restart { name: String, value: ValueId },
    Interrupt,
    BudgetExceeded(BudgetError),
}
```

**Benefits:**
- Normal R control flow (`stop()`, `tryCatch()`, `return()`, `break`, `next`) never unwinds the Rust stack.
- Unlocks `panic = "abort"` on Wasm and Android (significant binary-size win).
- Eliminates `vendor/wgpu` unwind patch (`Cargo.toml` `[patch.crates-io]`).
- Real Rust bugs (panics) stay cleanly separated from R-level errors.
- Unlocks Kani verification of evaluator and context transitions.

### 2.4 Unified Builtin & Native Routine Dispatch

Eliminate the 3 competing registration systems (`R_FunTab`, `eval/builtin`, `mainutils/essentials/registry.rs`) and the **189 duplicate `do_*` builtins**:

- **Single builtin table**: `BuiltinId` enum/index mapping each R primitive/internal directly to one canonical Rust handler:
  ```rust
  pub type BuiltinFn = fn(&mut Runtime, ValueId /* call */, &[ValueId] /* args */, ValueId /* env */) -> RResult<ValueId>;
  ```
- **Bundled `.Call`/`.C`/`.Fortran`/`.External` routines**: routed through one typed enum registry (`fn(&mut Runtime, &[ValueId]) -> RResult<ValueId>`). Zero `transmute_copy`, zero `c_void`, zero `extern "C"`, zero `rdynload.rs`. Unregistered foreign native calls return a clean R error.

### 2.5 Native Save/Load Format (`RBG1` — Rove Binary Graph v1)

Replace `mainutils/serialize/` (`10.8k LOC`), `saveload.rs` (`1k LOC`), and `xdr.rs` (`744 LOC`) with one ~400–600 LOC `#![forbid(unsafe_code)]` module:

- **Format**: Magic header `b"RBG1"` + optional `flate2` gzip wrapper + flat string table + flat node table (indexed `0..N`, preserving shared references and cycles cleanly via integer node indices) + root node index.
- **Validation**: Upfront bounds check on table counts and total decoded byte budget before allocating into `Heap` (naturally immune to decompression/allocation bombs).
- **Dependencies**: Drop `lzma-rs`, `lzma-rust2`, and `bzip2` from core serialization (keep only `flate2` pure Rust, or add `bzip2`/`lzma-rust2` only if explicitly needed by `connections` — never both `lzma-rs` and `lzma-rust2`!).
- **Bundled assets**: Convert `library/datasets/assets/*` and `library/portable_package/assets/*` via a one-time conversion script into `RBG1` blobs and retire the GNU `.rds`/`.rdx` readers.

### 2.6 Single Portable Graphics Stack

Retire the C-ported graphics/device duplication (`library/grdevices/{devps,stubs,...}`, `mainutils/graphics_ffi.rs`, `mainutils/engine.rs`, `unix/x11.rs`) and route all R `graphics` and `grid` primitives directly to `r-graphics-engine`'s `DrawTarget` trait (`#![forbid(unsafe_code)]`), keeping PDF/PNG/headless/GPU backends plugged into that one trait.

### 2.7 Target Workspace Layout (7 Core Crates + Platform Adapters)

Break the `598k LOC` `crates/rmath` monolith into a DAG of cleanly scoped crates (no circular dependencies, fast incremental rebuilds):

```
r-math          (renamed crates/nmath; pure Rust dist/special/rng, explicit &mut RngState/&mut MathState, zero TLS)
  ▲
r-core          (ValueId, Heap, GC, SymbolInterner, Parser/AST, Bytecode VM, Evaluator, Result<ValueId, RSignal>, #![forbid(unsafe_code)])
  ▲
  ├── r-base      (ONE canonical set of base R builtins: vectors, strings, io, datetime, s3/s4, RBG1 serialize, sys)
  ├── r-stats     (stats, splines, loess, pure-Rust faer linalg, appl optimizers)
  └── r-graphics  (unified r-graphics-engine + portable_plot + portable_grid + plotmath + graphics/grid builtins)
        ▲
r-runtime       (merges rmath::android + r-embed: ONE RSession, RValue, ValueHandle, resource limits, cancellation, virtual FS)
  ▲
  ├── r-host-cli            (desktop REPL/CLI)
  ├── r-wasm                (browser/Node wasm-bindgen boundary)
  ├── r-uniffi              (Kotlin/Swift worker-thread UniFFI boundary)
  └── r-device-vello-gpu    (optional GPU canvas/window presentation)
```

---

## 3. Phased Strangler-Fig Migration Roadmap

Every phase keeps `main` green and builds on the previous one. Avoid big-bang rewrites; ship small verified commits.

### Phase 1 — Hygiene, Dead-Code Deletion & Soundness Quick Wins
*Goal: cut ~30–50k LOC of dead C shims, fix immediate UB, remove duplicate dependencies and duplicate builtins.*

1. **Delete dead C portability shims & re-export stubs**: `trio/` (3.6k LOC), `alloca.rs`, `strncasecmp.rs`, `flexiblas.rs`, `strdup.rs`, `mkdtemp.rs`, `mainutils/inlined.rs`, `library/tcltk/`, and the 7 tiny `rmath/src/{dist,special,dpq,fprec,constants,error,utils}` wrappers; remove `libRmath.a` copy from `build.rs` and `staticlib`/`cdylib` from `crates/rmath/Cargo.toml`.
2. **Drop `fortran-backend` & C-library duplicates**:
   - Remove `fortran-backend` feature, `modules/lapack/{accelerate,init_win,veclib_g95c,lapack}.rs`, and Fortran linking in `build.rs`.
   - Remove `lzma-rs` (consolidate on `lzma-rust2` until Phase 2 decides whether `connections` keeps XZ).
   - Replace hand-ported `library/tools/{md5,sha256}.rs` with pure-Rust `md-5` / `sha2`.
   - Delete `intl/` (2k LOC, unused on Android/Wasm, barely referenced).
3. **Fix immediate soundness bugs**:
   - Replace `static mut environ` + `std::env::set_var` in `mainutils/essentials/runtime/sys/env.rs` with a session-owned `SessionEnv` map; remove hardcoded `/opt/homebrew/bin/R` discovery in `do_R_home`.
   - Replace `RenderPlotBackendGuard`'s lifetime-erasing `transmute` (`sexp/session/guards.rs:255`) with a closure-scoped API.
4. **Collapse the 189 duplicate `do_*` builtins**:
   - Audit every duplicate `do_*` against the active dispatch path, unify onto **one** canonical builtin registry (`R_FunTab` + `eval/builtin` + `essentials/registry` merged into one table), delete unreferenced duplicates (including `memory_main.rs` link-conflict copies), and remove module-wide `#[allow(dead_code)]`.

### Phase 2 — Retire C ABI, Wire-Format Bloat & Parallel Stacks
*Goal: remove `rdynload`/`dotcode` FFI, replace `.rds`/`.RData` with `RBG1`, unify graphics & `RSession` layers, evaluate `tre` replacement.*

5. **Retire dynamic C loading & `extern "C"` internal dispatch**:
   - Delete `rdynload.rs`, `unix/dynload.rs`, `registration.rs`, and foreign `.so` loading in `dotcode.rs`.
   - Convert all bundled `.Call`/`.C`/`.Fortran`/`.External` routines to safe Rust dispatch (eliminating `25+` `transmute_copy` calls and `177` `extern "C"` declarations).
   - Replace `R_alloc` (95 refs) and `vmaxget`/`vmaxset` (45 refs) with budgeted Rust `Vec` / `WorkspaceBuffer`.
6. **Replace `.rds`/`.RData`/`xdr.rs` (~12.6k LOC) with `RBG1`**:
   - Implement `#![forbid(unsafe_code)]` `RBG1` graph serializer/deserializer with upfront budget admission.
   - Convert `library/datasets/assets/*` and `library/portable_package/assets/*` to `RBG1` via a reproducible build/conversion script.
   - Delete `mainutils/serialize/`, `saveload.rs`, and `xdr.rs`.
7. **Unify `RSession` wrappers & rename `rmath::android`**:
   - Collapse `rmath::android::RSession` + `r-embed::RSession` into a single host session layer (no more 4-deep `RSession` nesting).
8. **Unify the graphics stack**:
   - Route all `graphics` and `grid` builtins through `r-graphics-engine`'s `DrawTarget` trait; delete `unix/x11.rs`, `mainutils/graphics_ffi.rs`, and legacy C device shims in `library/grdevices/` & `library/grid/`.
9. **Replace `tre/` (12.3k LOC, 373 `unsafe`)**:
   - Replace POSIX ERE compilation/matching with pure-Rust regex (POSIX-ERE adapter over `regex`/`fancy-regex`) and `agrep` with a small safe Rust Myers bit-parallel Levenshtein matcher; delete `crates/rmath/src/tre/`.

### Phase 3 — Replace `SEXP` with `ValueId` + Direct Generational `Heap`
*Goal: delete pointer laundering (`SLAB_META`/`SLAB_CACHE`), shrink handles, eliminate >85% of remaining `unsafe`.*

10. **Introduce `ValueId` + direct `Heap` API** alongside existing `sexp/` internals (converting `NodeLink` → `ValueId` directly instead of round-tripping through `SEXP`).
11. **Migrate `sexp/accessors`, `eval/`, `mainutils/`, and `library/` from `SEXP` to `ValueId`** subsystem by subsystem (starting with `sexp/envir`, `eval/eval`, `eval/bc_eval`, then base/stats builtins).
12. **Delete `SEXP`, `SLAB_META`, `SLAB_CACHE`, `LegacyProtectionStack`, and pinned `Cell<SexprecCore>` slabs**:
    - Convert `SexprecCore` → `Slot { ..., body: RObject }`.
    - Enforce `#![forbid(unsafe_code)]` on the entire heap & GC layer.

### Phase 4 — Explicit `&mut Runtime` + `Result<ValueId, RSignal>`
*Goal: zero ambient TLS, zero panic-as-control-flow, `panic = "abort"` ready.*

13. **Thread `&mut Runtime` explicitly** (completes and subsumes `rport-sg9a`):
    - Make `rmath-nmath` (`r-math`) accept explicit `&mut RngState` / `&mut MathState` (zero TLS).
    - Retire `CURRENT_INSTANCE`, `CurrentInstanceGuard`, and all remaining `thread_local!` blocks.
14. **Replace `panic_any(RError)` / `catch_unwind` with `Result<ValueId, RSignal>`**:
    - Convert evaluator, `RCNTXT` / `tryCatch` / `on.exit`, loops (`break`/`next`), `return()`, and builtins to return `RResult<ValueId>`.
    - Delete `vendor/wgpu` unwind patch (`[patch.crates-io]` in root `Cargo.toml`) and verify `panic = "abort"` builds for Wasm and Android.

### Phase 5 — Crate Split & Strict Lint Enforcement
*Goal: modular workspace DAG, fast incremental compiles, real Clippy safety net.*

15. **Split `crates/rmath` into `r-core`, `r-base`, `r-stats`, `r-graphics`, `r-runtime`** (and rename `crates/nmath` → `crates/r-math`), enforcing `#![forbid(unsafe_code)]` at the crate root of every non-FFI crate.
16. **Re-enable suppressed Clippy correctness lints** (remove the ~140 crate-wide `#![allow(clippy::...)]` entries in `lib.rs`, especially `cast_ptr_alignment`, `absurd_extreme_comparisons`, `while_immutable_condition`, `never_loop`, `mut_range_bound`, `missing_safety_doc`, and `dead_code`).

---

## 4. Verification Gates (Required for Every Milestone)

Before committing any milestone to `main`:

```bash
scripts/cargo_dev.sh test --workspace
scripts/cargo_dev.sh clippy --workspace --all-targets -- -D warnings
scripts/check_android_globals.sh
scripts/audit_safe_api.sh
# When R-visible behavior or evaluator/builtin wiring changes:
scripts/conformance_parity.sh --check --strict
# Track unsafe & LOC burn-down:
python3 scripts/unsafe_syntax_report.py --production
```

Update `docs/rust-r-port-architecture.md` and `docs/safe-api-audit.md` as each phase completes so documentation never drifts behind the code.
