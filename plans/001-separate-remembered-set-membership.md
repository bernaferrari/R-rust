# Plan 001: Separate remembered-set membership from GC reachability marks

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the STOP conditions occurs, stop and report; do not
> improvise. A reviewer maintains `plans/README.md`.
>
> **Drift check (run first)**:
> `git diff --stat 462f1280..HEAD -- rmath-rs/rmath/src/sexp/gengc.rs`
> If this file changed, compare the current-state excerpts below with live code.
> Any semantic mismatch is a STOP condition.

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.1`

## Why this matters

`RememberedSet::add` currently sets the same SEXP mark bit the collector uses
for reachability. The tracer returns immediately when it sees that bit, so it
never visits children of an old object remembered because of an old-to-young
write. A live young value can therefore be swept, producing user-visible data
loss and potential use-after-free. No compatibility score is credible while
this collector invariant is broken.

## Current state

- `rmath-rs/rmath/src/sexp/gengc.rs:128-138` treats `sxpinfo.mark()` as the
  current collection's visited bit and returns before traversing children.
- `gengc.rs:427-456` defines `RememberedSet { entries: Vec<SEXP> }`; `add`
  deduplicates by setting `sxpinfo.mark(true)`, and `clear` resets those marks.
- `gengc.rs:1012-1077` marks ordinary roots, then calls `mark_reachable` for
  remembered entries. Because their mark was set at barrier time, this is a
  no-op. Young, unmarked nodes are then freed.
- `gengc.rs:1306-1322` verifies only that a barrier inserts one entry; it never
  connects the pairlist/vector edge or proves the young child survives.

The relevant current pattern is:

```rust
if !(*obj).sxpinfo.mark() {
    (*obj).sxpinfo.set_mark(true);
    if self.entries.try_reserve(1).is_err() {
        return;
    }
    self.entries.push(obj);
}
```

This crate favors explicit, panic-free allocation paths in GC code. Preserve
that convention: no `unwrap`, no allocation while sweeping, and no new ambient
instance acquisition inside `*_in(instance: &mut RInstance)` functions.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Drift | `git diff --stat 462f1280..HEAD -- rmath-rs/rmath/src/sexp/gengc.rs` | no output |
| Targeted tests | `cargo test -p rmath sexp::gengc::tests -- --test-threads=1` | exit 0; all selected tests pass |
| Full library tests | `cargo test -p rmath --lib -- --test-threads=1 </dev/null` | exit 0; no failures |
| Format | `cargo fmt --check --all` | exit 0 |
| Lint | `cargo clippy -p rmath --all-targets --all-features -- -D warnings` | exit 0 |

The closed stdin on the full test command is deliberate: the existing console
read test blocks on an interactive PTY and is separately tracked as
`rport-jxfp.3.1`.

## Scope

**In scope** (the only source file to modify):

- `rmath-rs/rmath/src/sexp/gengc.rs`

**Out of scope**:

- `sexp/instance.rs` ambient aliasing; it is a separate architectural repair.
- Adding missing `package_namespace_cache` roots; Plan 003 follows this plan.
- Moving the collector or changing generation thresholds.
- Any public API or SEXP memory-layout change.

## Git workflow

- Work only in an isolated worktree on branch `advisor/001-gc-remembered-set`.
- Use conventional commits matching the repository, for example
  `fix(gc): separate remembered membership from marks`.
- Do not push or modify the user's branch.

## Steps

### Step 1: Give membership independent state

Replace mark-bit deduplication with data owned by `RememberedSet`. A small
address set plus the existing ordered entries vector is acceptable, provided
reserve failures cannot leave them inconsistent. A linear containment check is
also acceptable if justified with a measured/small remembered-set bound. The
ordinary SEXP mark bit must never be changed by `add`, `clear`, or membership
updates.

Keep `iter`, `len`, `entries_mut`, reference remapping, and `clear` internally
consistent. If entries are remapped, rebuild/update membership before a later
barrier can deduplicate.

**Verify**: `rg -n 'set_mark' rmath-rs/rmath/src/sexp/gengc.rs` and inspect the
matches. No match inside the `RememberedSet` implementation may be used for
membership.

### Step 2: Add survival regressions

Extend the existing `sexp::gengc::tests` module. Construct an old parent and a
young child, assign the child into an actually traced field, invoke the matching
write barrier, and keep only the old parent rooted. After `minor_gc`, assert the
young child was not freed/replaced and its generation/contents remain valid.

Cover at least:

1. a LISTSXP `carval` old-to-young edge;
2. a VECSXP element written through the vector barrier; and
3. duplicate barrier calls produce one remembered entry without modifying the
   parent's mark bit before collection.

Add a full-GC case only if full GC uses the remembered set in a way the new
storage changes. Tests must assert survival, not merely counts.

**Verify**: `cargo test -p rmath sexp::gengc::tests -- --test-threads=1` exits 0
and the new named survival tests are listed as passing.

### Step 3: Check remapping and session isolation

Exercise `update_remembered_set_in` and existing same-thread multi-session
tests. Ensure an entry remapped from `old` to `new` is found under `new`, not
`old`, and that clearing one instance does not affect another instance's
membership.

**Verify**: targeted tests, full library tests, format, and Clippy commands from
the command table all exit 0.

## Test plan

- Add named tests alongside `test_write_barrier_detects_old_to_young`.
- Assert the parent's mark is false immediately after barrier insertion.
- Assert the child survives and remains reachable from the parent after minor
  collection.
- Assert repeated barrier calls deduplicate.
- Preserve and run existing remap and session-local remembered-set tests.

## Done criteria

- [ ] No remembered-set method uses `sxpinfo.mark` for membership.
- [ ] Pairlist and vector old-to-young survival tests fail on `462f1280` and
  pass with the patch.
- [ ] Duplicate insertion, remapping, clear, and per-session isolation pass.
- [ ] Targeted tests, full rmath library tests, rustfmt, and strict Clippy pass.
- [ ] Only `rmath-rs/rmath/src/sexp/gengc.rs` is changed.

## STOP conditions

Stop and report if:

- The fix requires changing SEXP layout or a public FFI type.
- A membership allocation failure cannot be handled without silently losing a
  required old-to-young edge.
- A survival test requires rooting the young child independently; that would
  not prove the bug is fixed.
- Any in-scope current-state excerpt no longer matches live code.
- Verification fails twice after one reasonable correction.

## Maintenance notes

Every future remembered-set representation must keep reference remapping and
membership synchronized. Reviewers should scrutinize reserve-failure behavior
and make sure tests root only the parent. Plan 003 will add another GC root but
must not reintroduce mark-bit membership.

