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

### Owned header storage and allocation identity

Arena pages own initialized `Cell<SexprecCore>` arrays behind `Rc` storage.
Permanent environment, symbol, name, cons, and constructor allocations use
that same page implementation and share the arena's heap identity. Their
permanent policy keeps them rooted until explicit release or session teardown.
Permanent character, scalar, and string payloads are typed Rust cell arrays;
no header or payload is disowned into a raw allocation.

Arena vectors and character buffers also own initialized typed cell chunks.
Buffer transfer moves their owner by value; shared vector caches retain a
lease on that one allocation. Pending buffers retain the original header's
allocation identity and a byte-reservation lease, so recycling the header
cannot attach an old buffer to its replacement. Compact sequences fill a
private payload before publishing it to a header or a callback.

Process sentinels use `OnceLock`-owned atomic interior cells with native layout
assertions. Rust supplies their thread-safety traits without an unsafe assertion
for `SexprecCore`. Their canonical projections never escape into generic
metadata or element mutation: those setters leave shared sentinels unchanged.

`sexp/heap.rs` and `sexp/persistent.rs` forbid unsafe code. Each allocation has
an opaque identity comprising heap, page identity, slot, and generation. Slot
release invalidates its old identity before reuse; generation exhaustion retires
the slot. Dropping a page invalidates surviving metadata tokens without retaining
its header bytes. Collector occupancy, age, and epoch metadata use owned cells.
The legacy pointer directory indexes exact live page ranges without reading a
candidate pointer's header.

Raw `SEXP` pointers remain projections of those same physical headers during
the engine migration. Header unions, legacy payload views, graph fields,
evaluator access, and native compatibility still have audited unsafe paths.
Rust ownership of pages alone does not make those operations safe. Shared
backing storage is essential: moving a boxed page after publishing raw pointers
invalidates their aliasing provenance even when the bytes do not move.

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
- **Fallible claims and allocation-free release.** A claim reserves storage
  and checks its next generation before publishing a slot. Safe `try_*`
  entrypoints return capacity or generation errors without changing live
  roots. Release needs no new generation or allocation, including at
  exhaustion. Vacancies cannot match an old lease when a slot is reused.
- **The collector does not move live objects.** Root scans read both
  storages. Rust root occupants retain checked allocation identities; root
  snapshots end their storage borrow before traversal or callbacks. Guard
  cleanup also checks a weak owner-liveness witness before touching session
  storage. Old-to-young edges go through the remembered-set write barriers
  in `sexp/gengc.rs`.

Collector work carries the owning heap, exact allocation generation, and mark
epoch. It rederives each header projection from owned storage and rejects
unknown, foreign, or reclaimed graph nodes before header access. Its raw bridge
copies child values without mutable header or payload loans during marking.
Static sentinels require no mark writes. Permanent roots and reference rewrites
cover every owned persistent header, including ones absent from legacy lists.
The collection scope ends before notifications, including when tracing unwinds.

### Checked handles and managed roots

`RSession::sexp` validates pointer membership before dereferencing and installs
one managed root lease. Checked handles also retain their exact allocation
generation. Reclamation invalidates a handle permanently, even if its address
is reused; copied header reads and raw projection requests check that identity
before accessing interpreter memory. Clones share the root lease; the last
clone releases it.
Address lookup never authorizes dereferencing the caller's pointer. Successful
lookup derives a fresh projection from the owned cell, including for immutable
singletons. An input with the right address but no provenance therefore cannot
carry an invalid pointer into safe reads or writes.
Child accessors install independent leases against the original owner, so a
child remains live after its parent handle drops. Vector iterators retain the
parent and root yielded children. Guard cleanup uses the original owner even
when another session is active.

`RArena::sexp` validates membership and ties the handle to an arena borrow.
Safe builders retain their typed inputs and reject children from other arenas
or sessions. Child header snapshots validate the owning heap before reading a
projected pointer. Raw graph insertion and manual node freeing are unsafe: callers
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
`sexp/instance.rs`). The collector preserves object addresses. When an unreachable finalizer key
becomes ready, the collector traces the key's entire graph before sweeping.
Its newly retained young children participate in promotion and accounting;
cycles remain collectible after finalization unless explicitly resurrected.

GC notifications copy statistics and retain owned callback leases before
invoking user code. No borrow of `GcState` crosses a callback. A callback may
register another callback or collect again: new registrations participate in
the next notification, and nested collections update statistics without
recursively notifying the active callback set. An owned notification guard
resets this suppression flag on normal return and panic.

Each interpreter allocation owns a Rust liveness token that is invalidated
before its fields are destroyed. Notification loops and activation guards
retain weak observers of that identity, rather than dereferencing the owner
after arbitrary callback code. If a notification drops its ambient session,
later callbacks stop, collection skips owner-dependent cleanup, and activation
restores only a still-live previous interpreter, RNG and numerical state.
The weak control block distinguishes allocations even when addresses are
reused. It observes teardown; it does not extend the interpreter allocation's
lifetime or make a retained raw pointer safe to dereference. Raw entrypoints
retain their stated owner-lifetime contracts.

### Allocation admission and graph writes

Vector factories validate the type's union layout, convert the length with
`usize::try_from`, and validate the complete allocation layout before changing
the arena. Non-vector headers require their dedicated constructors; the GNU
compatibility bridge dispatches `LISTSXP` and `LANGSXP` to real node chains.
Unpublished vector and character payloads have RAII owners until the arena
adopts them.

Checked ALTREP factories install a temporary managed root before ending the
allocation's arena lend. Deferred collection can notify user callbacks as the
lend ends; rooting only after the factory returned allowed a nested full
collection to reclaim the fresh node first. Vector, string, metadata cons-cell
and checked compact-sequence construction now retain that temporary root
through notifications until the returned session handle owns its root lease.

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

Rooted Rust ALTREP classes are part of every build, including WebAssembly,
embedded builds with default features disabled, and the Kani configuration.
Integer, real, logical, raw, complex, string and list vectors share the checked
provider interface. A class descriptor is an interned symbol; data1, data2 and
the private expanded cache occupy a traced VECSXP in an internal attribute.
Class methods receive `AltrepContext` and return copied scalars or rooted
elements. Session-owned `Rc` class records are copied out before invocation,
so providers can allocate, collect and reenter R without a live instance,
arena, method-table or payload borrow. Owned operation guards reject recursive
element/expansion calls and serialization/duplication cycles, then reset after
errors or unwinds.

Expansion builds a rooted private vector and publishes only completed values.
`OwnedBuffer` owns registered allocations through RAII; checked header leases
allow the original and its expanded cache to share storage. Collection releases
only the final lease and accounts the allocation once. Cache handles may
outlive the original vector. Checked writes and translated pointer writes see
the same expanded values; borrowed string access expands before returning a
loan so its parent traces the child. Translated pointer-element reads retain
returned children in a sparse traced cache. Repeat and deferred classes use
traced data; deferred evaluation validates and roots a result before caching
it. Class type and cache policy are sampled once at registration under the
original owner; changing provider state cannot change the registered
representation. Logical length stays immutable after construction. Sequence,
repeat and deferred-evaluation providers and session compact-sequence helpers
use this Rust interface.

The Rust dispatch module rejects `unsafe` code; built-in class providers and
the registry forbid it. `altrep/registry.rs` owns immutable class records and
callback guards. Runtime state has an owned `Rc` lease with checked `RefCell`
borrows; class handles retain their record directly. Registering, looking up
or dropping a guard does not mutably borrow an interpreter field. The raw
bridge clones that state through one short documented field read.
`altrep/storage.rs` owns typed metadata fields, traced edges and buffer
publication; only a pending instance can set length. `altrep/bridge.rs` adapts
rooted handles to translated R execution. Storage and bridge modules contain
the audited unsafe operations; providers receive no mutable interpreter or
payload references. Context data reads return independently rooted handles
to current metadata, so cache writes are visible during the same callback and
throughout expansion. Errors propagate through `SexpResult`.

Serialization preserves R's portable object format. Known GNU compact
sequence, deferred-string and wrapper states remain readable. Safe classes
serialize as dense values with public attributes; ordinary deep and shallow
duplication also copies values and public attributes while excluding private
class metadata. Fresh copies remain rooted while child providers can collect.
The GNU C-shaped class handles, native method-registration tables and
Length/Elt/Duplicate/Inspect/Coerce callback adapter have been removed. Class
registration and construction use the checked Rust provider API.

Native and strict-provenance Miri gates exercise allocation denial, collection,
aliasing, reentrancy and recovery. The nightly Rust-class gate runs without
feature switches on native and 32-bit targets, alongside compact-vector,
shared-buffer, notification and owner-teardown tests. Each milestone records
the validation actually run.

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
