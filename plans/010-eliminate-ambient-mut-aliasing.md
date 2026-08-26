# Plan 010: Eliminate aliased ambient mutable instance access

> **Executor instructions**: This is a staged soundness migration. Execute each
> step in order in an isolated worktree, keep behavior unchanged, and stop if a
> safe ownership proof cannot be expressed. A reviewer maintains the index.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- rmath-rs/rmath/src/sexp/instance.rs rmath-rs/rmath/src/sexp/memory.rs rmath-rs/rmath/src/sexp/gengc.rs rmath-rs/rmath/src/sexp/protect.rs`

## Status

- **Priority**: P0
- **Effort**: XL
- **Risk**: HIGH
- **Depends on**: Plans 001 and 003
- **Category**: memory safety / architecture
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.8`

## Why this matters

`with_current_instance` safely exposes `&mut RInstance` from a thread-local raw
pointer. Nested calls are explicitly allowed and only counted. A caller can
therefore retain one mutable reference while a nested helper manufactures a
second overlapping mutable reference to the same instance, which violates
Rust's aliasing contract even on one thread. The pattern is concrete in arena,
protection, evaluation, and GC helpers. Passing tests cannot make undefined
behavior safe.

## Current state

- `sexp/instance.rs:684-696` increments a diagnostic depth and always creates
  `&mut *ptr`; it neither rejects nor prevents nesting.
- `sexp/instance.rs:764-790` exposes that operation through safe closure APIs.
- `sexp/memory.rs:746-758` acquires the raw pointer independently for arena
  access, including while an outer instance borrow can remain live.
- GC safe points and translated helpers mix explicit `*_in(instance)` calls
  with ambient protection/allocation helpers, creating real nested paths.
- The only regression checks that depth resets after panic; it deliberately
  does not test that a second mutable acquisition is impossible.

## Scope

**In scope**:

- `rmath-rs/rmath/src/sexp/instance.rs`, `memory.rs`, `protect.rs`, `gengc.rs`,
  `session.rs`.
- The smallest set of evaluator/mainutils callers needed to thread an existing
  `&mut RInstance` into new `_in` variants.
- Soundness and session-isolation tests; a compile-fail test for any safe API
  boundary that replaces ambient mutable access.

**Out of scope**:

- Changing SEXP layout, moving to a different collector, or changing R output.
- Hiding aliasing behind `UnsafeCell`, a mutex, reference-counted raw pointers,
  or a depth counter that still yields overlapping `&mut` values.
- A wholesale rewrite of translated modules.

## Steps

### Step 1: Make a second ambient mutable acquisition impossible

Inventory every ambient acquisition and classify it as a top-level boundary or
a nested helper. Introduce an actual dynamic exclusive-borrow guard at the TLS
boundary which rejects re-entry before constructing `&mut RInstance`; it must
reset during unwinding. Do not ship this alone if normal tests still re-enter.

Add tests for sequential access, unwind recovery, separate sessions, and nested
access rejection. The guard is a migration oracle, not the final ownership
model.

### Step 2: Thread explicit instance borrows through nested hot paths

Convert nested allocation, protection, GC, output, error, and evaluation paths
to `*_in(&mut RInstance, ...)` functions. Derive field borrows from that one
reference so the compiler can enforce disjoint/reborrow lifetimes. Ambient
wrappers may remain only at true top-level/C compatibility boundaries and must
immediately delegate to an `_in` function.

Run the full library after each subsystem; do not silence guard failures by
dropping and reacquiring raw pointers.

### Step 3: Narrow or remove the safe ambient mutable API

Once all nested calls are gone, make the remaining raw-pointer acquisition a
private unsafe boundary owned by session activation, or replace it with a
scoped owner token that cannot escape. Safe code must not be able to manufacture
an arbitrary `&mut RInstance` from TLS. Remove the diagnostic-only wording and
depth escape hatch.

### Step 4: Prove behavior and tooling gates

Run:

```sh
cargo test -p rmath --lib -- --test-threads=1 </dev/null
cargo test -p rmath --test for_loop_gc -- --test-threads=1
cargo fmt --check --all
cargo clippy -p rmath --all-targets --all-features -- -D warnings
cargo miri test -p rmath sexp::instance::tests -- --test-threads=1
```

If Miri cannot build an FFI-heavy target, extract a minimal owner/borrow model
test that it can run; do not claim Miri coverage for code it skipped.

## Done criteria

- [ ] Nested ambient mutable acquisition cannot yield two `&mut RInstance`s.
- [ ] Arena, protection, GC, and evaluator nesting uses explicit `_in` paths.
- [ ] The remaining TLS raw pointer is confined to a documented unsafe session
      boundary and cannot be borrowed mutably through a safe general API.
- [ ] Multi-session, unwind, GC stress, full tests, formatting, and lint pass.
- [ ] Miri covers the replacement ownership primitive or an explicit limitation
      is documented without treating it as proof.

## STOP conditions

Stop if the proposed design still creates overlapping mutable references, if a
callback requires re-entrancy without an explicit reborrowable context, if the
change alters session selection during evaluation, or if behavior gates fail
twice after one narrow correction.

## Maintenance notes

New translated wrappers should have a thin ambient entry and an explicit
`*_in(instance)` implementation. Runtime depth counters are useful assertions;
they are not substitutes for Rust ownership.
