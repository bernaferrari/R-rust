# Plan 002: Make root-generation updates transactional and prove the production root table

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the STOP conditions section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 16223ff7..HEAD -- crates/rmath/src/sexp/protect.rs crates/rmath/src/sexp/session.rs docs/rust-r-port-architecture.md`
> On a mismatch with the excerpts below, STOP.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: plans/001-kani-lane-and-leaf-proofs.md
- **Category**: security
- **Planned at**: commit `16223ff7`, 2026-09-23

## Why this matters

`RootTable` is the generational root map the rest of the runtime already uses. Its interesting safety properties are local: a stale `(index, generation)` pair must not clear or replace the current occupant; `restore` must drop unmanaged roots created since a checkpoint and must keep managed ones; legacy `UNPROTECT` must not touch the table.

The generation counter already refuses to wrap, via `checked_add(1).expect("root generation exhausted")`. The updates around that call are not transactional. In `claim`, the new SEXP is stored, and on the fresh-slot path the entry is pushed, before `next_gen` runs. In `release`, the entry is nulled and removed from `managed` before `next_gen` runs. A panic at `u64::MAX` therefore leaves a live pointer under the previous generation (so the stale handle now names the new value) or a null slot whose generation still matches the releaser. Kani can hit `u64::MAX` immediately. A long random test will not.

This plan makes one generation reservation happen before any other field changes, then proves bounded operation sequences against the production methods. It does not prove `ProtectGuard`, the collector, or embedder handles.

## Current state

`RootTable` in `crates/rmath/src/sexp/protect.rs:218` fields:

- `entries: RefCell<Vec<SEXP>>`
- `generations: RefCell<Vec<u64>>`
- `next_generation: Cell<u64>`
- `free_list: RefCell<Vec<usize>>`
- `managed: RefCell<HashSet<(usize, u64)>>`

State-changing methods to preserve behavior for, all in that impl:

- `checkpoint` (`:240`) reads `next_generation`
- `retain_managed` (`:244`)
- `restore` (`:252`) releases slots whose generation is `>= checkpoint` and whose pair is not in `managed`
- `next_gen` (`:270`) `checked_add(1).expect("root generation exhausted")`
- `claim` (`:284`) free-list reuse or push
- `release` (`:325`) generation-checked tombstone, free list, tail collapse
- `reprotect` (`:359`)
- `truncate` (`:390`) index cut; not what `ProtectScope` calls
- `clear` (`:401`)

`LegacyProtectionStack` (`:126`) is a separate `RefCell<Vec<SEXP>>`. `pop_count`, `remove_topmost`, and `truncate` touch only that vec.

`ProtectScope` (`crates/rmath/src/sexp/session.rs:354`) saves `legacy_protect.len()` and `root_table.checkpoint()`, and on drop calls `legacy_protect.truncate` and `root_table.restore`. The field is named `root_depth` but the value is a generation, not a length. The comment at `:365` currently says "Root-table entry depth". Fix that comment when you touch the file. Do not change `ProtectScope` to call `truncate` on the root table.

Unsafe non-transactional order today:

```288:296:crates/rmath/src/sexp/protect.rs
if let Some(index) = self.free_list.borrow_mut().pop() {
    // ...
    entries[index] = s;
    let generation = self.next_gen();
    generations[index] = generation;
```

```300:314:crates/rmath/src/sexp/protect.rs
entries.push(s);
let index = entries.len() - 1;
let generation = self.next_gen();
// generations.push happens after next_gen
```

```338:340:crates/rmath/src/sexp/protect.rs
self.managed.borrow_mut().remove(&(index, slot.generation));
entries[index] = std::ptr::null_mut();
generations[index] = self.next_gen();
```

Existing tests already cover single scenarios. Do not delete them. They are the regression net for the reorder:

- stale release / reprotect: `test_slot_reuse_rejects_old_token`, `stale_reprotect_cannot_replace_reused_slot`
- legacy stack does not alias the table: `test_legacy_unprotect_never_truncates_root_slots`, `test_root_release_never_shifts_legacy_entries`
- managed root survives a public scope: `public_scope_does_not_revoke_returned_root`

`docs/rust-r-port-architecture.md:78-83` is stale. It says the stack is a plain `Vec`, `Vec::remove` shifts indexes, LIFO is required, and a generational table is roadmap. The module docs at `protect.rs:11-33` are the accurate description: two storages, arbitrary-order root release, legacy LIFO only for `LegacyProtectionStack`.

SEXP values in a harness are opaque `*mut ()` pointers. Never dereference them. The invariant is about indexes and generations, not about the pointee.

Kani command and version come from `scripts/cargo_kani.sh` after plan 001. Do not install a different Kani.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Root unit tests | `scripts/cargo_dev.sh test -p rmath --lib sexp::protect` | exit 0 |
| Kani harness | `scripts/cargo_kani.sh -p rmath --no-default-features --harness root_table_ops_preserve_invariant` | `VERIFICATION:- SUCCESSFUL` |
| Exhaustion harness | `scripts/cargo_kani.sh -p rmath --no-default-features --harness root_table_exhaustion_is_atomic` | `VERIFICATION:- SUCCESSFUL` |
| Doc drift | `rg -n "generation-based handle table" docs/rust-r-port-architecture.md` | no matches |

## Scope

**In scope**

- `crates/rmath/src/sexp/protect.rs`
- `crates/rmath/src/sexp/session.rs` (comment on `ProtectScope.root_depth` only)
- `docs/rust-r-port-architecture.md` (the protect-stack subsection only, lines 62–83 as they stand at `16223ff7`)
- Kani harnesses next to `RootTable`, gated `#[cfg(kani)]`

**Out of scope**

- `ProtectGuard`, `RootedSexp`, `with_guard_owner`, thread-local current instance
- `gengc.rs`, arena allocation, `reserve_slot_or_fail` policy beyond "do not call the allocator in the exhaustion harness"
- `RootTable::truncate` behavior changes. It may appear in a harness only as the operation it already is.
- Embed `ValueHandle` (`crates/r-embed/src/session.rs`)
- Fuzz targets
- Making exhaustion return `Result` to R callers. The panic remains the session-fatal policy. What changes is that the panic happens before the table is mutated.

## Git workflow

- Branch: `advisor/002-root-table`
- Two commits: (1) transactional generation reservation plus the architecture paragraph and the `ProtectScope` comment; (2) Kani harnesses. Imperative sentence with a period.
- Do not push unless the operator instructed it.

## Steps

### Step 1: Reserve a generation before any other mutation

Replace the internal `next_gen` shape so a failed reservation changes nothing.

Target shape:

```rust
fn try_reserve_generation(&self) -> Result<u64, ()> {
    let generation = self.next_generation.get();
    let next = generation.checked_add(1).ok_or(())?;
    self.next_generation.set(next);
    Ok(generation)
}
```

`claim` and `release` call this and panic with `root generation exhausted` only when it returns `Err`, and only before they write `entries`, `generations`, `free_list`, or `managed`.

Concrete order:

- **Reuse path in `claim`:** reserve first; on `Err`, push the popped free-list index back (or pop only after reservation succeeds) and panic. On `Ok`, then write `entries[index]` and `generations[index]`.
- **Push path in `claim`:** reserve first; on `Err`, panic with both vecs the same length they started at. On `Ok`, then `try_reserve` / `push` the entry and the generation together. If `reserve_slot_or_fail` panics after a generation was reserved, that generation is skipped rather than reused. Skipping a generation is acceptable. Writing an entry without a generation is not. Do not try to roll back a failed `try_reserve` by decrementing the counter if that reintroduces wrap complexity; document the skipped generation in a one-line comment.
- **`release`:** generation-check the slot first (stale release stays a no-op and must not consume a generation). For a live slot, reserve the tombstone generation before nulling the entry or editing `managed` / `free_list`. On `Err`, panic with the occupant, its generation, and `managed` unchanged.

Do not change successful-path results: same indexes, same generation sequence starting at 0, same stale no-op, same tail collapse, same `restore` rule.

**Verify**: `scripts/cargo_dev.sh test -p rmath --lib sexp::protect` → exit 0.

### Step 2: Add a deterministic exhaustion unit test

Drive `next_generation` to `u64::MAX` through a test-only method `#[cfg(test)] fn set_next_generation_for_test(&self, value: u64)` that is not public. One test calls `claim` and asserts the panic payload contains `root generation exhausted`, and that `len`, the free list, and `generation_at` of any pre-seeded slot are unchanged. One test does the same for `release` of a live slot: after catching the panic with `catch_unwind`, the entry pointer and generation still match the pre-call state.

`catch_unwind` in a unit test is allowed. Do not use it inside a Kani harness. Kani does not model unwinding; the Kani exhaustion harness should call `try_reserve_generation` (make that helper `pub(crate)`) and assert `Err` leaves `next_generation` unchanged. The panic wrappers stay thin.

**Verify**: `scripts/cargo_dev.sh test -p rmath --lib exhaustion` → exit 0, at least 2 tests.

### Step 3: Correct the architecture paragraph

Rewrite `docs/rust-r-port-architecture.md` lines 62–83 so they match `protect.rs`:

- Legacy stack: LIFO, count-based, `Rf_unprotect` only.
- Root table: stable indexes, generation, free list, any drop order, stale handle is a no-op.
- The collector does not move live objects. Delete the phrase "heap edges into moved values". Old-to-young edges use the remembered set.
- State that generation exhaustion panics before mutating the table.

Do not rewrite the rest of the document.

Fix the `ProtectScope` comment so `root_depth` is described as the `checkpoint()` generation, not an entry count.

**Verify**: `rg -n "Vec::remove" docs/rust-r-port-architecture.md` finds no protect-stack claim. `rg -n "moved values" docs/rust-r-port-architecture.md` finds nothing in the protect section.

### Step 4: Bounded invariant harness on the real `RootTable`

`#[cfg(kani)]` in `protect.rs`. Construct `RootTable::new()`. Do not construct `RInstance`.

Bounds: at most 4 slots, at most 8 operations. Operations are a symbolic enum: `Claim`, `Release(i)`, `Reprotect(i)`, `Retain(i)`, `Restore(cp)`, `Checkpoint`. SEXP arguments are distinct non-null integer tokens cast to `SEXP`, never dereferenced. Start `next_generation` at 0. In this harness `kani::assume` that every `try_reserve_generation` would succeed (the counter stays below `u64::MAX` because 8 claims cannot exhaust it; do not also assume it inside the exhaustion harness).

Invariant after every op, including the initial state:

- `entries.len() == generations.len()`
- free-list indexes are unique and `< entries.len()`
- a free-list index's entry is null (after a successful release; a stale free-list index discarded by `claim` is the exception to assert: `claim` must not resurrect an out-of-range index)
- for any two live handles recorded by the harness, different generations at the same index do not occur
- `release` of a handle whose generation is not `generation_at` leaves every entry and every generation unchanged
- `reprotect` of a stale handle leaves entries unchanged
- `restore(cp)` keeps every managed pair with generation `>= cp`, and releases every unmanaged pair with generation `>= cp`
- `LegacyProtectionStack::pop_count` on a sibling stack leaves a `RootTable` bit-identical. One harness step is enough; do not model `RInstance`.

`kani::cover`: a reuse of a freed slot, a stale release, a managed slot surviving `restore`, an unmanaged slot cleared by `restore`.

**Verify**: `scripts/cargo_kani.sh -p rmath --no-default-features --harness root_table_ops_preserve_invariant` → `VERIFICATION:- SUCCESSFUL`.

### Step 5: Exhaustion harness and a mutant

Harness `root_table_exhaustion_is_atomic`: set the counter to `u64::MAX` via the test helper or by making the helper available under `cfg(any(test, kani))`. Assert `try_reserve_generation` is `Err` and a following guarded `claim` / `release` does not change vectors. If the panic path cannot be called from Kani, proving `try_reserve_generation` plus the unit tests from step 2 is the accepted split. Say so in the harness comment. Do not weaken the unit tests.

Mutant, not committed: delete the generation comparison in `release` on a local edit, run the invariant harness, observe failure, restore the comparison.

**Verify**: exhaustion harness successful; mutant not present in `git diff`.

## Test plan

- Unit: claim at `u64::MAX` panics and restores any popped free-list index; release at `u64::MAX` panics with the occupant intact.
- Existing protect tests listed above still pass.
- Kani: 8-step invariant, restore managed-vs-unmanaged, stale release, legacy stack disjoint, exhaustion reservation.
- `kani::cover` on reuse, stale release, managed survival, unmanaged cleanup.

## Done criteria

- [ ] `claim` and `release` do not write entries, generations, free list, or managed before a successful generation reservation
- [ ] `scripts/cargo_dev.sh test -p rmath --lib sexp::protect` exits 0
- [ ] Both Kani harness names print `VERIFICATION:- SUCCESSFUL`
- [ ] The stale "generational table is roadmap" paragraph is gone
- [ ] `ProtectScope` still calls `restore(checkpoint)` and the comment says generation, not length
- [ ] No harness constructs `RInstance` or dereferences a SEXP
- [ ] `plans/README.md` row 002 is `DONE`

## STOP conditions

Stop and report back if:

- `RootTable` fields or `ProtectScope::drop` no longer match the excerpts.
- Transactional reservation changes any existing protect test result.
- The invariant harness needs more than 4 slots or 8 operations to avoid a counterexample. A counterexample is a bug or a wrong invariant. Do not raise the bound to hide it.
- Kani cannot see `pub(crate)` methods from a `#[cfg(kani)]` module in the same file. Do not make the table public to work around that.
- A proof depends on stubbing `claim` or `release`.
- You need to change `panic = unwind` or catch panics inside Kani.

## Maintenance notes

- New root operations must update the invariant harness's op enum in the same change. A state change that is not in the enum is an unproved path.
- `truncate` is not scope restoration. Callers that want scope behavior call `restore`.
- Managed versus unmanaged is the product rule: safe RAII roots call `retain_managed`; raw `protect` roots die at `restore`. Do not "simplify" that by retaining every slot.
- Guards and GC visit roots later. This plan does not license removing Miri or `gc_torture_stress.sh`.
