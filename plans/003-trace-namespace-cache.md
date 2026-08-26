# Plan 003: Trace and remap cached package namespace SEXPs

> **Executor instructions**: Execute each step and verification in order. Stop
> on any STOP condition; do not improvise. A reviewer maintains the index.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- rmath-rs/rmath/src/sexp/gengc.rs rmath-rs/rmath/src/mainutils/essentials.rs rmath-rs/rmath/src/sexp/instance.rs`

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: `plans/001-separate-remembered-set-membership.md`
- **Category**: bug
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.5`

## Why this matters

`RInstance.package_namespace_cache` owns raw namespace environment pointers,
but the collector neither marks nor remaps them. A namespace retained only by
the cache can be swept while the cache keeps its address, turning a later `::`
lookup into stale-pointer access. The fix must cover both marking and reference
updates so adding/removing moving-GC hooks cannot recreate the bug.

## Current state

- `sexp/instance.rs:376-377` stores
  `HashMap<String, (PathBuf, SEXP)>`.
- `mainutils/essentials.rs:6917-6967` allocates a namespace environment,
  inserts it into the cache, and later returns the cached pointer.
- `sexp/gengc.rs:210-303` enumerates instance roots but never visits cache
  values.
- `sexp/gengc.rs:740-840` remaps all known instance roots but omits cache
  values.

Follow the paired convention already visible in `gengc.rs`: every instance
field marked in `mark_instance_roots` must have a corresponding update in
`update_instance_roots_in` when it contains a mutable SEXP pointer.

## Commands

| Purpose | Command | Expected |
|---|---|---|
| GC tests | `cargo test -p rmath sexp::gengc::tests -- --test-threads=1` | exit 0 |
| Package tests | `cargo test -p rmath package -- --test-threads=1` | exit 0 |
| Full library | `cargo test -p rmath --lib -- --test-threads=1 </dev/null` | exit 0 |
| Format/lint | `cargo fmt --check --all && cargo clippy -p rmath --all-targets --all-features -- -D warnings` | exit 0 |

## Scope

**In scope**:

- `rmath-rs/rmath/src/sexp/gengc.rs`
- One existing package/namespace test module in
  `rmath-rs/rmath/src/mainutils/essentials.rs`, only if the end-to-end
  regression cannot live in the GC tests.

**Out of scope**:

- Changing `RInstance` cache representation or package-loading policy.
- Eviction, cache size limits, native packages, or lazy-load support.
- Remembered-set changes already handled by Plan 001.

## Git workflow

- Branch `advisor/003-namespace-gc-root` in an isolated worktree.
- Conventional commit: `fix(gc): retain cached package namespaces`.
- Do not push or modify the user's branch.

## Steps

### Step 1: Add the missing paired root operations

In `mark_instance_roots`, iterate cache values and mark each namespace SEXP.
In `update_instance_roots_in`, update the mutable SEXP within every cache
value. Do not mark paths or clone/rebuild the entire cache unnecessarily.

**Verify**: a focused unit test can insert a SEXP in the cache and observe its
mark/remap behavior without another independent root.

### Step 2: Add a cache-only survival regression

Load or construct a pure-R namespace, arrange for the cache to be its only
non-heap root, force full GC, then retrieve/use it again. Assert a representative
export or `package::name` lookup still works. The test must fail when the two
new root operations are removed.

**Verify**: targeted GC and package test commands exit 0.

### Step 3: Run the full affected gate

Run full rmath library tests with closed stdin, format, and strict Clippy.

## Done criteria

- [ ] Cache namespace SEXPs are marked and remapped in paired functions.
- [ ] A namespace retained only by the cache survives full GC and remains usable.
- [ ] No package policy or public API changes.
- [ ] Targeted/full tests, format, and strict Clippy pass.
- [ ] Only in-scope files change.

## STOP conditions

Stop if Plan 001 is not complete, the cache is not the only root in the new
test, the fix requires changing cache/public types, or any verification fails
twice.

## Maintenance notes

Any new SEXP-bearing `RInstance` field must be added to both root enumeration
and remapping. A future improvement should encode that pairing in one visitor,
but this plan intentionally avoids an architectural rewrite.

