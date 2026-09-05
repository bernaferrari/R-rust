# Rust R Port Architecture

This port keeps R's semantics and source shape where that helps conformance,
but the ownership model is Rust-first. C-shaped entrypoints are compatibility
shells; Rust-facing code should use session-owned state, typed `Sexp` handles,
and explicit embedding boundaries.

## Layers

1. **Raw compatibility layer**

   `SEXP`, `SEXPTYPE`, `Rf_*`, `.Internal` shims, and translated routines live
   here. This layer may stay close to upstream R so future C changes can be
   replayed and compared. Raw pointers should not cross into Android, UniFFI, or
   new Rust APIs.

2. **Instance and memory layer**

   `RInstance` owns mutable interpreter state: arena, environments, protect
   stack, preserve stack, RNG state, caches, output capture, path policy,
   graphics state, and evaluator control state. `RArena` owns allocation and
   `Sexp<'a>` is an owner-scoped handle proving that a raw pointer is either
   owned by the active session/arena or is an immutable sentinel.

3. **Rust runtime layer**

   New runtime code should prefer APIs such as `RSession::sexp`,
   `RSession::eval_sexp`, `RSession::eval_sexp_in`, `EvalContext`, and
   `eval_expr`. `Rf_eval` remains for ported internals that still speak raw
   `SEXP`, but it should delegate inward rather than owning evaluator policy.

4. **Embedding layer**

   `rmath::android`, `r_embed`, and `r_uniffi` return owned Rust values:
   `RValue`, `String`, `Vec<u8>`, and records. They must not expose `SEXP` or
   depend on mutable process-global state. Android hosts configure app-private
   paths explicitly and run each session on its owning worker thread.

## Ownership Rules

- Every mutable R runtime operation needs an active `RSession`/`RInstance`.
- No new mutable process global should be added for Android-facing behavior.
- Raw `SEXP` wrapping belongs at owner boundaries: `RArena::sexp` or
  `RSession::sexp`.
- Public Rust APIs should accept and return `Sexp<'_>` or owned values, not raw
  pointers.
- Raw C ABI compatibility functions should be thin shells around typed Rust
  helpers.
- Session state should be movable into `RInstance` before a feature becomes
  Android-facing.
- Cross-thread hosts should use one `RSession` per worker/thread. Sharing a live
  session across workers is outside the current safety contract.

## Object Ownership and GC Safety

This section documents the ownership model as actually shipped, in
`rmath-rs/rmath/src/sexp/`. The safe facade built on top of it is
experimental: its exact proof coverage is tracked in
`docs/conformance.md`, and its remaining gaps are listed in the
README's known-gaps ledger.

### Protect stack (`sexp/protect.rs`)

The port of R's `PROTECT`/`UNPROTECT` mechanism, owned by the active
`RInstance`:

- **Indexed and typed.** Alongside the count-based
  `protect_sexp`/`ProtectGuard`, `protect_sexp_with_index` returns an
  `IndexedProtectGuard` over a typed `ProtectionSlot` — the shape of
  upstream's `PROTECT_WITH_INDEX`/`REPROTECT` pair. Guards are
  owner-bound: each remembers the `RInstance` it was created against
  (stored as an address with exposed provenance) and unprotects against
  that instance even if the ambient current instance has switched.
- **Generation-aware rooting.** A protected handle is a GC root: the
  generational collector's root scan (`with_protected_objects`) reads
  the protect stack, and heap edges into moved values go through the
  remembered-set write barriers in `sexp/gengc.rs`.
- **LIFO drop-order contract.** The stack is a plain `Vec`; releasing a
  slot (`Vec::remove`) shifts later indices, so guards must drop in
  reverse creation order — the natural order for RAII scopes. A
  generation-based handle table that pins slots permanently and removes
  the LIFO constraint is roadmap; until then the stack semantics stay
  as upstream R's.

### `RootedSexp`: RAII rooting with a write barrier

`RootedSexp::root` clones the (non-`Copy`) handle, protects the clone
on creation, and unprotects on `Drop`; reads deref to the guarded
handle. Embedders never juggle protect/unprotect bookkeeping by hand.

A value that may be *replaced* during evaluation (a grown vector, a
re-promised PROMSXP) must be refreshed through the slot's write
barrier — `RootedSexp::reprotect` or
`IndexedProtectGuard::reprotect_sexp` — which retargets the protected
slot and the guarded handle together. Never refresh a value by mutating
a raw pointer in place.

### The Miri audit

Nightly CI (`cargo +nightly miri test -p rmath sexp::`) runs a bounded
subset of the `sexp::` tests under Stacked Borrows checking, in default
permissive-provenance mode, with `-Zmiri-ignore-leaks`: the runtime
deliberately allocates immortal persistent objects (base symbols,
`CHARSXP` payloads, primitive metadata) that live until process exit,
mirroring upstream R, and the leak check would flag them without
weakening the aliasing check that is the point of the job.

**What was tested:** the safe ownership layer — memory, memory_ext,
object, and protect tests — with **216 tests proven clean** today; the
subset is re-run nightly and grows as tests are added. ~167 `sexp::`
tests are not yet Miri-run.

**What was found:** five structural Stacked-Borrows violations plus ~60
violations in lending closures, fixed in commit `a6fe04e3`:

- instance re-acquisition retagged a stored `Box` root tag instead of
  re-deriving `&mut RInstance` through `with_exposed_provenance`;
- `with_arena` lends of `&mut inst.arena` were invalidated by instance
  re-acquisition — the lend is now exposed for wildcard re-basing in
  `with_arena_in`, and arena slab pages are raw allocator allocations;
- `ProtectScope` stored the protect-stack address behind a borrow-like
  tag (now stored with exposed provenance);
- `ContextGuard::instance_ptr()` borrowed through a foreign tag (now
  re-derived from the guard's own tag);
- ~60 `with_arena` closures ignored the arena lend or held stale
  borrows across re-acquisition (migrated to `with_active`).

**What was fixed:** the owner-bound guard design above — guards
re-derive their owning instance from exposed provenance instead of
holding borrow tags — plus the migration of every lend-ignoring closure.

**What remains:** the evaluator and library layers have no Miri
coverage, leak auditing is disabled, and the `r-embed` safe facade as a
whole is unaudited. ALTREP is disabled pending the external-pointer
redesign (unsound representation).

### Handle discipline

The rules every safe API in the crate follows:

1. **Handles are non-`Copy`.** `Sexp` moves on assignment; a `Copy`
   handle would let a stale alias legally survive an in-place mutation
   of the same R object — precisely the aliasing-undefined-behavior
   class this crate forbids. A `compile_fail` doctest pins the
   use-after-move error. The full `SexpRef`/`SexpMut` borrow split is
   roadmap; non-`Copy` handles with by-value mutation are the shipped
   interim.
2. **Mutation is by value.** Accessors consume the handle (clone first
   to keep it), so a mutation can never happen behind an outstanding
   shared alias.
3. **Cloning is explicit and shallow.** `clone()` produces a second
   cheap handle over identical R memory, never a deep copy — and the
   same no-alias-across-mutation rule applies to the clone.
4. **Write barriers go through `reprotect`.** Values replaced during
   evaluation are refreshed through protected slots, never by editing
   raw pointers in place.
5. **Rooting is explicit.** Holding a handle does not root the object:
   the generational GC may collect anything only reachable from Rust
   locals once an R evaluation re-enters. Retain a value across a GC
   point with `RootedSexp` or a protect guard.

### Embedding boundary

`r-embed` and `r_uniffi` sit on top of this model: owned Rust values
out, no raw `SEXP` in user-facing signatures, one session per worker
thread. The facade is experimental: it inherits the GC discipline
verified in the sections above but has no independent audit yet.

Long-lived values cross this boundary as `ValueHandle`s: `Copy`
`(session, slot, generation)` ids with no reference into the arena. The
value itself stays rooted in a reserved engine-internal environment;
`read_handle`/`write_handle` return session-borrowed guards
(`ReadGuard` snapshots the owned value, `WriteGuard::set`/`update`
rebind it), and use-time validation turns foreign-session or stale-slot
handles into errors instead of undefined behavior. This is the
host-facing half of the generation-aware-rooting roadmap item; the
crate-internal protect-stack half is documented above.

### Crate-scale split (deferred)

Decision: the core stays a modular monolith in the single `rmath` crate
(plus the already-split `rmath-nmath` and the embedding crates
`r-embed`/`r-uniffi`). A finer split into `r-translated-core` /
`r-runtime` / `r-safe` is deferred, not rejected.

Why deferred:

- **Single-owner borrow surface in `sexp/`.** Handles (`Sexp<'a>`),
  owner-bound protect guards, and the `with_exposed_provenance`
  re-derivation patterns assume one crate can see the arena, instance,
  and provenance plumbing together (see the Miri audit above).
  Splitting now would freeze premature `pub` boundaries through unsafe
  internals that are still being reshaped.
- **Generation-table prerequisite.** The protect stack is still a plain
  `Vec` with a LIFO drop-order contract; the roadmap generation-based
  handle table that pins slots permanently has not landed. A crate
  boundary cut today would lock in the index-shifting stack semantics.
- **No second consumer yet.** All embedding paths (`r-embed`,
  `r-uniffi`, `android`) sit on the same `RInstance`/`RSession`. There
  is no independent consumer that needs a smaller dependency subset, so
  a split would add workspace churn with no user.

Intended boundaries when revisited:

- **`r-translated-core`** — faithful translations that stay close to
  upstream and may speak raw `SEXP` internally: `eval/`, `library/`,
  `mainutils/`, `modules/`, and the C-port leaves (`appl`, `dist`,
  `dpq`, `special`, `fprec`, `tre`, `trio`, `unix`, `graphapp`, `intl`,
  `xdr`, `tzone*`, `rng`, `constants`, `error`, `utils`).
- **`r-runtime`** — session-owned state and collection: `sexp/`
  ownership core (`memory`, `memory_ext`, `instance`, `session`,
  `context`, `envir`, `env_hash`, `symbol`, `protect`, `gengc`, `init`,
  `globals`, `constructors`, `accessors`, `attrib_core`, `output`).
- **`r-safe`** — the only API new Rust and embedding code should touch:
  `sexp/object` (`Sexp`, `SexpMut`, the future `SexpRef`,
  `RootedSexp`, `builder`) plus the typed entrypoints
  (`RSession::sexp`, `RSession::eval_sexp*`, `EvalContext`,
  `eval_expr`).

Revisit trigger: a second embedding consumer that needs a stable subset
without the translated core, or the safe API stabilizing — the shipped
`SexpRef`/`SexpMut` borrow split plus the generation handle table. Until
then, keep new code behind the typed `Sexp`/session boundary inside the
monolith instead of pre-splitting.

## Evaluator Shape

The evaluator is Rust-shaped at its primary boundary:

- `EvalContext<'a>` binds an owner-scoped environment.
- `eval_expr(expr, env)` owns cancellation and visibility setup.
- `RSession::eval_sexp*` proves pointer ownership before evaluation.
- `Rf_eval` converts raw pointers and delegates to `eval_expr`.

This keeps the port faithful inside the evaluator while avoiding a C-shaped API
for new Rust code.

## Android Policy

Android embedding is app-owned and session-owned:

- `configure_android_paths(app_files_dir, cache_dir, bundled_library_dir)` sets
  the user library, cache/temp directory, and bundled package library for one
  session.
- `.libPaths()`, `find.package()`, `library()`, `require()`, `tempdir()`, and
  `tempfile()` resolve through `RInstance::path_policy`.
- `render(code, width, height)` returns PNG bytes from a headless renderer and
  evaluates plot data on the worker session.
- Resource limits for evaluation depth, cooperative wall-clock checks, arena
  bytes, and arena node count are host-configurable per session.
- `system()` is disabled on Android. Host builds keep it enabled for stock-R
  parity and conformance checks.
- Native package loading through `useDynLib()` is rejected until an Android
  host-owned native-library policy exists.
- Native entrypoints (`.Call`, `.C`, `.Fortran`, `.External`, `dyn.load`, and
  `library.dynam`) fail loudly at the R boundary. Ported base/library internals
  should be exposed as Rust evaluator builtins rather than hidden behind C ABI
  compatibility.
- Mutable-global additions must pass `scripts/check_android_globals.sh`.

## Upstream Sync Workflow

1. Find or add the target row in `docs/upstream-port-map.tsv`.
2. Import or diff the target upstream R C file under `r-source`.
3. Identify the behavioral unit: parser, evaluator, builtin, math routine,
   device, package helper, or platform shim.
4. Keep translated control flow close to upstream inside raw compatibility
   modules when that improves reviewability.
5. Move ownership, allocation, state, cancellation, paths, and output into
   `RInstance`/`RSession` instead of preserving process globals.
6. Add or update the typed Rust entrypoint first, then make the C-shaped shim
   delegate to it.
7. Add focused Rust tests for the typed API and Android/embedding tests when the
   behavior crosses FFI boundaries.
8. Add conformance cases when behavior is user-visible R semantics.
9. Run the parity and Android gates before committing.

The sync-mode vocabulary and checked source map live in
`docs/upstream-port-map.md`.

## Conformance Gates

Run these for changes that affect evaluator, SEXP ownership, Android, or
embedding behavior:

```bash
RUSTFLAGS=-Awarnings cargo check -p rmath -p r-embed -p r-uniffi
RUSTFLAGS=-Awarnings cargo test -p rmath --lib -- --test-threads=1
RUSTFLAGS=-Awarnings cargo test -p r-embed -p r-uniffi -- --test-threads=1
RUSTFLAGS=-Awarnings cargo test -p rmath --doc
scripts/conformance_parity.sh
RUSTFLAGS=-Awarnings cargo check --target aarch64-linux-android -p rmath -p r-embed -p r-uniffi
scripts/check_android_globals.sh
git diff --check
```

For narrow non-embedding changes, run the subset that covers the touched layer,
but do not skip `scripts/conformance_parity.sh` when R-visible behavior changes.
