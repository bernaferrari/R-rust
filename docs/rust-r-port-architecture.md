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
   Checked `Sexp<'a>` factories validate membership and retain the owner.
   Legacy raw factories are unsafe and require manual liveness/rooting proofs.

3. **Rust runtime layer**

   New runtime code should prefer APIs such as `RSession::sexp`,
   `RSession::eval_sexp` and `RSession::eval_sexp_in`. `EvalContext` and
   `eval_expr` are unsafe internal dispatchers with explicit owner/root contracts. `Rf_eval` remains for ported internals that still speak raw
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
`crates/rmath/src/sexp/`. The safe facade built on top of it is
experimental: its exact proof coverage is tracked in
`docs/conformance.md`, and its remaining gaps are listed in the
README's known-gaps ledger.

### Protect stack (`sexp/protect.rs`)

The port of R's `PROTECT`/`UNPROTECT` mechanism, owned by the active
`RInstance`:

- **Two storages.** `LegacyProtectionStack` is the count-based LIFO
  stack behind `Rf_protect` / `Rf_unprotect`. `RootTable` is the
  generational table behind Rust guards: stable slot indexes, a free
  list, and a generation on every claim. Legacy `UNPROTECT` never
  truncates root-table slots.
- **Arbitrary-order root release.** Releasing a root tombstones that
  slot and recycles its index. A stale `(index, generation)` handle is
  a no-op, so it cannot evict the current occupant. Guards are
  owner-bound: each remembers the `RInstance` it was created against
  and unprotects against that instance even if the ambient current
  instance has switched.
- **Scope restore is a generation checkpoint.** `ProtectScope` saves
  `RootTable::checkpoint()` and later calls `restore`. Managed roots
  (safe RAII guards) survive. Unmanaged raw `protect` roots allocated
  since the checkpoint are released. `RootTable::truncate` is a
  separate index cut and is not what scope exit uses.
- **Generation exhaustion is session-fatal and transactional.** The
  counter uses `checked_add`. If it cannot advance, `claim` and
  `release` panic with `root generation exhausted` before writing
  entries, generations, the free list, or managed identities.
- **The collector does not move live objects.** Root scans read both
  storages. Old-to-young edges go through the remembered-set write
  barriers in `sexp/gengc.rs`.

### Checked handles and managed roots

`RSession::sexp` validates pointer membership before dereferencing and installs
one managed root lease. Clones share that lease; the last clone releases it.
Child accessors install independent leases against the original owner, so a
child remains live after its parent handle drops. Vector iterators retain the
parent and root yielded children. Guard cleanup uses the original owner even
when another session is active.

`RArena::sexp` validates membership and ties the handle to an arena borrow.
Safe builders retain their typed inputs and reject children from other arenas
or sessions. Raw graph insertion and manual node freeing are unsafe: callers
must establish graph ownership and exclude surviving handles and payload loans.

`RootedSexp` remains useful for an explicitly replaceable root. `get` checks its
slot generation; it has no `Deref`. `reprotect` retargets that root and its stored
handle. `unroot` releases the additional slot; a checked session handle keeps
its automatic lease.

### Copies, mutation, and payload loans

Safe reads copy elements or return owned Rust values. `SexpMut::try_from_checked`
accepts checked mutable owners and rejects unknown raw handles and immutable
singletons. Its setters check type, bounds, and child ownership. Read-handle
clones may coexist because safe reads never lend Rust payload references.

Borrowed slices and strings are private to the object implementation. Its
unsafe loans borrow the handle itself and exclude payload mutation and R
execution until the reference dies. Runtime consumers use copied elements,
owned snapshots, or copies into caller-owned buffers. The former session view
callback was removed. `SexpMut::from_owned` remains unsafe for legacy raw
handles. Moving or cloning a handle does not prove uniqueness.

`OwnerToken<'session>` binds checked wrapping, incremental pairlist builders,
and collection to a live session without creating a whole-instance Rust
reference. Collection checks that this owner is active before dispatching
callbacks. Raw owner-pointer collection entrypoints are unsafe; translated
core bridges still carry explicit lifetime, rooting and reentry obligations.

Unscoped raw wrappers and evaluator entrypoints are unsafe. Functions retaining
the historical `_safe` suffix may return typed errors while still requiring an
unsafe owner/rooting contract. Safe runtime entry should go through `RSession`;
the embedding crates expose owned values and validated `ValueHandle` ids.

### Arena lends and collection

Mutable arena lends reject reentry. Allocation-time GC is deferred until the
lend ends. Direct collection requests also defer before touching an arena with
a live mutable lend; quiescent session processing services the pending request.
No whole-instance Rust borrow may survive R reentry (the P1/P2 rules in
`sexp/instance.rs`). The collector preserves object addresses.

### Allocation admission and graph writes

Vector factories validate the type's union layout, convert the length with
`usize::try_from`, and validate the complete allocation layout before changing
the arena. Non-vector headers require their dedicated constructors; the GNU
compatibility bridge dispatches `LISTSXP` and `LANGSXP` to real node chains.
Unpublished vector and character payloads have RAII owners until the arena
adopts them.

`R_alloc` buffers retain both their raw allocation and a reservation against
their original session's byte budget. Watermark resets and instance teardown
drop both together. Nonempty allocation failures raise `RError` before ported
callers can write through a null pointer. Reservations during an arena lend
use that owner's ledger without borrowing the arena again, including when
another owner's lend is nested inside it. Watermarks encode opaque indices
with provenance-free pointers and are never dereferenced.

Checked string, generic and expression vector writes record old-to-young
edges in the handle's original session even when a different session is
active. Bounds/type/ownership checks and the fallible remembered-set insertion
precede publication. If insertion fails, the graph is unchanged and the setter
returns an allocation error. Standalone arenas have no generational collector.
Legacy raw setters have an infallible barrier contract; a failed insertion is
fatal OOM because some callers have already published their edge.

Checked compact-sequence mutations materialize the backing buffer in the
original owner, with an activation guard that restores the ambient instance,
RNG and math state. The owner keeps its budget and payload after another
session is destroyed.

Compact payload expansion reserves its complete byte cost before allocation
or zero-fill, including when a foreign arena lend is nested inside its owner.
Unpublished buffers have an RAII owner and bookkeeping capacity is reserved
before pointer publication. Failure leaves the formula intact: checked writes
return a typed error, while raw `DATAPTR`/`INTEGER`/`REAL` requests for nonempty
vectors raise `RError` before translated callers can dereference null. Numeric
comparison reads compact elements without requesting expansion. Empty
vectors retain their existing pointer convention. Expansion converts logical
lengths with `usize::try_from` to reject 32-bit truncation.

The opt-in `altrep` feature provides rooted Rust classes for integer, real,
logical, raw, complex, string and list vectors. A class descriptor is an
interned symbol; data1, data2 and the private expanded cache occupy a traced
VECSXP in an internal attribute. They never occupy a numeric buffer or hold a
native Rust pointer. Class methods receive `AltrepContext` and return copied
scalars or rooted elements. Tables are session-owned `Rc` values, copied out
before invocation; callbacks can allocate, collect and reenter R without a live
instance, arena, method-table or payload borrow. Rust-owned operation guards
reject recursive element/expansion calls and serialization/duplication cycles,
then reset after errors or unwinds.

Expansion builds a rooted private vector and publishes only completed values.
`OwnedBuffer` owns registered allocations through RAII; checked header leases
allow the original and its expanded cache to share storage. Collection releases
only the final lease and accounts the allocation once. Cache handles may
outlive the original vector. Pointer writes and checked writes see the same
expanded values; borrowed string access expands before returning a loan so its
parent actually traces the child. Native pointer-element reads retain returned
children in a sparse, traced cache. Repeat and deferred classes use traced data;
deferred evaluation validates and roots a result before caching it. Class type
and cache policy are sampled once at registration under the original owner;
changing provider state cannot change the registered representation. Logical
length is assigned by a consuming construction handle and stays immutable.

The Rust dispatch module rejects `unsafe` code, and built-in class providers
forbid it. `altrep/registry.rs` owns one class record including optional native
methods and Rust callback guards. `altrep/storage.rs` owns typed metadata fields,
traced edges and buffer publication; only a pending instance can set length.
`altrep/bridge.rs` adapts rooted handles to translated R execution and documents
the contracts for raw callers. These private modules contain the audited unsafe
operations; providers receive no mutable interpreter or payload references.
Context data reads return checked, independently rooted handles to current
metadata, so a cache write is visible during the same callback and throughout
bulk expansion. They propagate root-allocation failure with `SexpResult`;
providers use `context.data1()?` and `context.data2()?`. This changes the opt-in
Rust provider interface from the earlier snapshot getters.

Serialization falls back to dense values with public attributes, and ordinary
duplication excludes internal class metadata. Native Length/Elt, duplicate,
inspect and coerce adapters validate and root inputs and results, ending table
borrows before calling C code. Native callbacks retain their unsafe contract;
this is not a complete GNU C API/ABI implementation (custom serialized state,
DLL reload, and the full optional method/optimization table remain outside this
implementation). The feature stays opt-in; defaults are unchanged. Default
compact sequences remain available without it. Native and strict-provenance
Miri gates exercise allocation denial, collection, aliasing and recovery.

GraphApp buffers reject size overflow before allocating or reallocating and
align their payloads for object pointers, including platforms where C long is
narrower than a pointer. Failed growth retains the existing buffer. Image
construction admits the complete pixel size, and palette replacement copies
before releasing the old buffer so an aliased source remains valid.

These budgets account for object data and admitted workspaces, not total
process RSS or every renderer/library allocation. The allocation nightly gate
also exercises vector admission on a 32-bit target to prevent long-length
truncation regressions. The object gate covers owner-specific barriers,
collection after lease release, and an injected insertion failure.

### Miri evidence

The nightly object gate runs `--lib sexp::object::` with strict provenance and
isolation enabled. The earlier ownership refinement recorded 68 passing object
tests plus two passing checked-mutation tests under the same flags. These cover
managed lease cleanup, child and iterator liveness, foreign-owner rejection,
GC between incremental pairlist appends, and collection deferral during an
arena lend. A separate strict-provenance collector stress gate exercises 64
protected vectors through 20 real collections and slab reuse.

The allocation hardening on 2026-10-02 recorded 74 passing object tests and a
separate later compact-owner regression, five transient-allocation tests, GNU
pairlist construction, and GraphApp allocator/image checks under strict
provenance. Vector admission and GraphApp allocation checks also passed under
Miri on i686. The integrated native checkout passed 156 focused cases; its
full-base-runtime object fixture was excluded from this focused run because
of expensive debug initialization. The Wasm embedding build, Clippy and the
app-facing safe-API audit passed. These are scoped regressions, not a full GNU
R compatibility or interpreter safety certification.

Leak checking is disabled for these gates because persistent runtime objects
are deliberately retained. Full base-library and default-package heap stress
also has native coverage. These runs establish evidence for specific paths;
they do not prove the whole interpreter, allocator, or collector sound. Broader
evaluator/module coverage and resource accounting remain separate work.

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
  `sexp/object` (`Sexp`, `SexpMut`, `SexpRef`,
  `RootedSexp`, `builder`) plus the typed entrypoints
  (`RSession::sexp`, `RSession::eval_sexp*`). Unsafe internal dispatchers
  remain behind this boundary.

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
- Unregistered native entrypoints (`.Call`, `.C`, `.Fortran`, `.External`,
  `dyn.load`, `dyn.unload`, and `library.dynam`) fail loudly at the R boundary. Symbols
  registered as in-tree Rust ports run as those ports, including `.C` routines
  and `.Fortran` routines such as `hclust` and `dtrco`. Host libraries stay
  unloaded.
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
