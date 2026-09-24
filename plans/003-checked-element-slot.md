# Plan 003: Prove the checked string and list element decision on the code that ships

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the STOP conditions section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 16223ff7..HEAD -- crates/rmath/src/sexp/accessors.rs`
> On a mismatch with the excerpts, STOP.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: plans/001-kani-lane-and-leaf-proofs.md
- **Category**: security
- **Planned at**: commit `16223ff7`, 2026-09-23

## Why this matters

String and list element access already rejects a bad tag, a null data pointer, and an out-of-range index before pointer arithmetic (`checked_element_slot`). That is the check that closed a Miri out-of-bounds write. It still panics through `panic_any(RError)`, which Kani cannot unwind, and it still trusts the header length to describe the allocation. The second trust is an explicit assumption, not something this plan pretends to prove.

The scalar helpers `INTEGER_ELT`, `REAL_ELT`, and `LOGICAL_ELT` do not perform that check. In release builds the type test is a `debug_assert`, and the index is added with no length test. That matches GNU R's unchecked element macros. Proving them "safe for every index" by adding checks would change a widely used C-shaped contract. This plan proves the checked decision and leaves the unchecked macros alone.

## Current state

```819:851:crates/rmath/src/sexp/accessors.rs
unsafe fn checked_element_slot(x: SEXP, i: R_xlen_t, string_only: bool) -> *mut SEXP {
    // tag must be STRSXP when string_only, else VECSXP/EXPRSXP/STRSXP/BCODESXP
    // panic_any(RError) on a bad tag
    // panic_any(RError) when i < 0 || i >= vecsxp_length || DATAPTR is null
    // otherwise DATAPTR.cast::<SEXP>().add(i as usize)
}
```

Callers include `STRING_ELT` and the vector-element getter/setter just above this function. The setter also calls `gengc::vector_write_barrier`. Do not pull the barrier into the proof.

Unchecked contrast, which you must not modify:

```927:937:crates/rmath/src/sexp/accessors.rs
pub unsafe fn INTEGER_ELT(x: SEXP, i: c_int) -> c_int {
    // null / alignment checks, then
    debug_assert_sexptype(x, &[SEXPTYPE::INTSXP, SEXPTYPE::LGLSXP]);
    *data.add(i as usize)
}
```

`Sexp::try_integer_elt` in `crates/rmath/src/sexp/object/vector.rs` is the safe API and is out of scope except as a comment pointing at it.

Kani is invoked only through `scripts/cargo_kani.sh` from plan 001, with `--no-default-features`. Unwinding assertions stay on.

Existing unit tests around `accessors.rs:1095` build stack `SexprecCore` values. Match that style if you need a runtime test. The Kani harness should not need a live node.

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Accessor tests | `scripts/cargo_dev.sh test -p rmath --lib sexp::accessors` | exit 0 |
| Kani | `scripts/cargo_kani.sh -p rmath --no-default-features --harness element_slot_decision_complete` | `VERIFICATION:- SUCCESSFUL` |

## Scope

**In scope**

- `crates/rmath/src/sexp/accessors.rs`
- A `#[cfg(kani)]` module in that file

**Out of scope**

- `INTEGER_ELT`, `REAL_ELT`, `LOGICAL_ELT`, `SET_INTEGER_ELT`, `SET_LOGICAL_ELT`, and their `debug_assert_sexptype` lines
- `vector_write_barrier`, `gengc.rs`, allocators
- Sorting (`do_sort`, `sort.rs`) and subscript (`subscript.rs`)
- Changing panic text that tests already match, except splitting the predicate from the panic as specified below

## Git workflow

- Branch: `advisor/003-element-slot`
- One commit for the predicate extraction plus harness. Imperative sentence, period at the end.
- Do not push unless the operator instructed it.

## Steps

### Step 1: Extract the decision, keep the panic at the wrapper

Add a pure function in `accessors.rs` and call it from `checked_element_slot` before `data.add`:

```rust
pub(crate) struct ElementSlotReject {
    pub kind: ElementSlotRejectKind, // BadTag or BadIndex
}

pub(crate) fn element_slot_decision(
    tag: SEXPTYPE,
    string_only: bool,
    length: i64, // the type vecsxp_length already returns; if that type is not i64, use the real type
    index: i64,
    data_is_null: bool,
) -> Result<(), ElementSlotReject>
```

Use the real length and index types from `vecsxp_length` and `R_xlen_t`. Do not invent a second integer width. If `R_xlen_t` is not `i64`, the signature uses `R_xlen_t`.

`Ok(())` iff all of the following hold:

- tag is `STRSXP` when `string_only` is true
- tag is `VECSXP`, `EXPRSXP`, `STRSXP`, or `BCODESXP` when `string_only` is false
- `index >= 0 && index < length`
- `data_is_null` is false

`checked_element_slot` maps `Err` to the existing `panic_any(RError)` messages and maps `Ok` to `data.add(index as usize)`. The pointer add stays in the unsafe wrapper. The pure function contains no `unsafe` and no panic.

Do not change which tags are legal. `BCODESXP` stays legal for the non-string path because the current `matches!` allows it.

**Verify**: `scripts/cargo_dev.sh test -p rmath --lib sexp::accessors` → exit 0.

### Step 2: Prove the decision over its whole finite tag domain

Harness `element_slot_decision_complete`:

- Symbolic `tag` as the `SEXPTYPE` discriminant, `string_only: bool`, `length` and `index` in a range that covers negatives, zero, `length == index`, `length == index + 1`, and a value above `index`. Bound the integers to `-2..=4` plus the two extremes `i64::MIN` / `i64::MAX` if the real type is `i64` (use the type's MIN/MAX). If the solver chokes on MIN/MAX, keep the small range and add two concrete `#[test]`s for MIN and MAX instead of deleting the cases.
- Symbolic `data_is_null`.
- Assert `Ok` exactly on the predicate in step 1.
- Assert every `Err` distinguishes bad tag from bad index when both are wrong: bad tag wins, matching today's order (tag is checked first).
- `kani::cover`: legal STRSXP read, legal VECSXP read, BCODESXP accepted only when `string_only` is false, index `-1`, null data, tag `INTSXP` rejected.

No SEXP is built. No allocator is called.

**Verify**: `scripts/cargo_kani.sh -p rmath --no-default-features --harness element_slot_decision_complete` → `VERIFICATION:- SUCCESSFUL`.

### Step 3: Mutant

Locally force `element_slot_decision` to return `Ok(())` for `index == length`. The harness must fail. Restore the comparison before committing.

**Verify**: mutant fails Kani; `git diff` does not contain `index == length` treated as success.

### Step 4: Comment the unchecked boundary

Above `INTEGER_ELT`, add a four-line comment: this helper does not check the index or the release-mode type; callers must already have a legal INTSXP/LGLSXP index; the checked list/string path is `checked_element_slot`; the safe object path is `Sexp::try_integer_elt`. Do not add a runtime check.

**Verify**: `rg -n "fn INTEGER_ELT" -A 8 crates/rmath/src/sexp/accessors.rs` shows the comment and the same `data.add` body.

## Test plan

- Kani harness `element_slot_decision_complete` with the covers listed above.
- Existing accessor tests unchanged and passing.
- Concrete unit tests for `i64::MIN` and `i64::MAX` only if Kani could not include them.
- Mutant: `index == length` accepted, proof fails, mutant not committed.

## Done criteria

- [ ] `checked_element_slot` calls `element_slot_decision` before `data.add`
- [ ] The Kani harness prints `VERIFICATION:- SUCCESSFUL` with unwinding assertions still enabled
- [ ] `scripts/cargo_dev.sh test -p rmath --lib sexp::accessors` exits 0
- [ ] `INTEGER_ELT` still has no length check
- [ ] No GC, arena, or session type is referenced from the harness
- [ ] `plans/README.md` row 003 is `DONE`

## STOP conditions

Stop and report back if:

- `checked_element_slot`'s legal tags are not exactly STRSXP / VECSXP / EXPRSXP / STRSXP / BCODESXP as excerpted.
- Extracting the predicate changes an existing accessor test or panic message that a test matches on, and you cannot keep the message.
- The proof requires stubbing `DATAPTR` or building a full `RInstance`.
- You find yourself editing `INTEGER_ELT` to satisfy the harness.
- Kani reports a counterexample on the predicate. That means the predicate does not match the wrapper. Fix the extraction, do not shrink the tag domain.

## Maintenance notes

- Any new legal tag in `checked_element_slot` must update `element_slot_decision` and the harness together.
- Header `length` is still not tied to the real allocation size. Do not describe this proof as memory-safety of arbitrary SEXP pointers.
- Write barriers on `SET_STRING_ELT` remain a Miri / remembered-set test obligation (`docs/runtime-hardening.md` describes the string write-barrier regression).
