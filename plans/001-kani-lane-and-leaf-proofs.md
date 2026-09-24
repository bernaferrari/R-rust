# Plan 001: Stand up a separate Kani lane and prove two production leaves

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the STOP conditions section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md`.
>
> **Drift check (run first)**: `git diff --stat 16223ff7..HEAD -- rust-toolchain.toml crates/rmath/src/eval/bytecode.rs crates/rmath/src/mainutils/serialize/core.rs Cargo.toml`
> If any in-scope file changed since this plan was written, compare the
> Current state excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `16223ff7`, 2026-09-23

## Why this matters

There is no Kani setup anywhere in the repository. The project toolchain is Rust 1.96.0 (`rust-toolchain.toml`). Kani 0.68.0, released 2026-09-16, verifies with its own compiler pin `nightly-2026-08-21` (https://github.com/model-checking/kani/releases/tag/kani-0.68.0). Editing the workspace pin so Kani can run would silently move every other build off the version CI and the oracle are tracked against.

The first proofs should be functions that are already pure, already shipped, and already have unit tests. That shows the lane works and that a failing mutant fails the proof. Two such functions exist:

- `validate_gnu_bytecode_stream` walks a GNU bytecode integer stream using `GNU_BC_OPERAND_WIDTHS`.
- `BinaryReader::read_vector_length` rejects a vector length the remaining bytes cannot hold, before any R allocation.

## Current state

- `rust-toolchain.toml` pins `channel = "1.96.0"`. Do not change it.
- Workspace edition is 2024 (`Cargo.toml` `[workspace.package]`). Kani's nightly is new enough for that edition. If it is not, STOP.
- No file under the repo contains `kani` except unrelated vendor text. Confirmed by search while this plan was written.
- `fuzz/` is a detached workspace so sanitizer builds do not gate the main resolver. Copy that pattern only if a detached package is required to keep the toolchain pin. Prefer a script first.
- Local builds go through `scripts/cargo_dev.sh`, which runs `cargo` and then prunes binaries. Use it for ordinary tests. Kani gets its own script.
- Nightly Miri is already a separate toolchain lane (`.github/workflows/nightly.yml`: `cargo +nightly miri test -p rmath sexp::` with `-Zmiri-ignore-leaks`). Do not fold Kani into that job or its flags.

Bytecode framing today:

```256:287:crates/rmath/src/eval/bytecode.rs
pub fn validate_gnu_bytecode_stream(code: &[c_int]) -> Result<(), String> {
    let Some(&version) = code.first() else {
        return Err("GNU R bytecode stream is empty".to_string());
    };
    // version window, then for each opcode:
    //   width = GNU_BC_OPERAND_WIDTHS[opcode]
    //   reject unknown opcode and truncated operand tails
    //   pc = end
    Ok(())
}
```

The width table is `GNU_BC_OPERAND_WIDTHS` at `bytecode.rs:242`. `GNU_BC_OPCODE_COUNT` is 129. Version bounds are `GNU_BC_MIN_VERSION` / `GNU_BC_MAX_VERSION` in the same file. The function returns `Err` for a bad frame and `Ok(())` when every instruction lands on `code.len()`. It does not check constant indexes or stack depth. Those live in `validate_gnu_adapter_impl` and are out of scope.

Deserializer length check today:

```410:422:crates/rmath/src/mainutils/serialize/core.rs
fn read_vector_length(&mut self, binary_element_bytes: usize) -> Result<i32, String> {
    let len = self.read_i32()?;
    let count = usize::try_from(len).map_err(|_| "read error: negative vector length".to_string())?;
    let minimum = if self.ascii_body { 1 } else { binary_element_bytes };
    if count > self.remaining() / minimum {
        return Err("read error: truncated vector payload".into());
    }
    Ok(len)
}
```

`minimum == 0` divides by zero. The harness must include that input. If the function panics, return `Err` before the division. A deserializer must not panic because a caller passed a zero element width. Do not change accepted results for `minimum >= 1`.

Commit messages in this repo are imperative sentences with a period, for example `Default na.action to na.omit.`

## Commands you will need

| Purpose | Command | Expected on success |
| --- | --- | --- |
| Install pinned verifier | `cargo install --locked kani-verifier --version 0.68.0 && cargo kani setup` | `cargo kani --version` prints 0.68.0 |
| Ordinary unit tests | `scripts/cargo_dev.sh test -p rmath --lib bytecode::` | exit 0 |
| Kani | `scripts/cargo_kani.sh` once step 2 has defined it | prints `VERIFICATION:- SUCCESSFUL` for each new harness and a non-zero status for the mutant |

Reference, if a flag is unclear: https://model-checking.github.io/kani/usage.html and https://model-checking.github.io/kani/tutorial-loop-unwinding.html. Leave unwinding assertions enabled. Do not pass `--unwinding-assertions` off.

## Scope

**In scope**

- `scripts/cargo_kani.sh` (create)
- `crates/rmath/src/eval/bytecode.rs` (harness module only, plus a shared width walk if step 4 needs it)
- `crates/rmath/src/mainutils/serialize/core.rs` (zero-width reject, if required, and a harness)
- `crates/rmath/Cargo.toml` only if Kani needs a dev-dependency or feature. Do not add `kani` to default features.
- `.github/workflows/` only if step 6 adds a manually triggered or nightly job. Do not add Kani to the pull-request `ci.yml` test job.

**Out of scope**

- `rust-toolchain.toml`
- `eval_gnu_adapter`, `bcEval`, stack-effect validation, private `OP_*` bytecode
- `RootTable`, GC, argument matching, LAPACK, faer
- `panic = abort`, Cargo profile changes, Miri flags
- Enabling ALTREP or new workspace members unless the STOP condition forces a detached `kani/` package

## Git workflow

- Branch: `advisor/001-kani-lane`
- Commits: one for the runner script, one for the bytecode harness, one for the length harness (and the zero-width `Err` if you add it). Message style matches `git log`: imperative, period at the end.
- Do not push or open a pull request unless the operator instructed it.

## Steps

### Step 1: Install Kani without moving the workspace pin

Install `kani-verifier` 0.68.0 and run `cargo kani setup`. Confirm `rust-toolchain.toml` still says `1.96.0`.

**Verify**: `git diff -- rust-toolchain.toml` prints nothing. `cargo kani --version` contains `0.68.0`.

### Step 2: Record a runner that does not override the pin

Create `scripts/cargo_kani.sh`. It runs Kani against `rmath` with default features disabled:

```bash
scripts/cargo_kani.sh -p rmath --no-default-features --harness <name>
```

Requirements for the script:

- Executable bit set.
- Does not edit `rust-toolchain.toml`.
- Does not enable `renderplot-device` or the default `rust-backend` / faer feature.
- Leaves `--unwinding-assertions` at Kani's default (enabled).
- Echoes the exact `cargo kani` line it runs.
- Exits with Kani's status.

Try, in order, and keep the first one that verifies a tiny `#[cfg(kani)]` harness:

1. `cargo kani` inside the repo.
2. `RUSTUP_TOOLCHAIN=nightly-2026-08-21 cargo kani ...` if the project pin rejects Kani's sysroot.

If both compile the crate with stable 1.96.0, or either fails before a harness runs, STOP. The fallback, which you may then implement, is a detached package `kani/` copied from `fuzz/Cargo.toml`'s empty `[workspace]`, with its own `rust-toolchain.toml` containing only:

```toml
[toolchain]
channel = "nightly-2026-08-21"
```

That package depends on `rmath` with `default-features = false`. Do not add it to the root `[workspace].members`.

**Verify**: `scripts/cargo_kani.sh -p rmath --no-default-features --harness kani_trivial_ok` prints `VERIFICATION:- SUCCESSFUL`. `git diff -- rust-toolchain.toml` is empty.

### Step 3: Prove bytecode framing on the production function

Add `#[cfg(kani)]` harnesses in `bytecode.rs` (or a sibling `bytecode_kani.rs` included only under `cfg(kani)`). They must call `validate_gnu_bytecode_stream`, not a copy.

Properties, all with unwinding assertions left on:

1. **Accept implies a complete partition.** For a symbolic stream of length at most 12, `Ok(())` implies a second walk using only `GNU_BC_OPERAND_WIDTHS` visits every index exactly once as either the version word, an opcode, or an operand of the preceding opcode, and finishes at `code.len()`.
2. **Reject is forced.** An opcode outside `0..GNU_BC_OPCODE_COUNT`, or a width that runs past `code.len()`, yields `Err`.
3. **Empty stream.** `[]` yields `Err`.
4. **Reachability.** `kani::cover` an accepted 1-instruction stream and a truncated stream, so a harness that assumes the stream empty cannot pass.

Bound the stream with `kani::any()` on a fixed `[i32; 12]` plus a symbolic length `0..=12`. Do not use `Vec` nondeterminism. Do not call the executor.

Keep the existing unit tests. Add one unit test that the width table has length `GNU_BC_OPCODE_COUNT` if that assertion is not already present.

**Verify**: `scripts/cargo_kani.sh -p rmath --no-default-features --harness validate_gnu_stream_partition` → `VERIFICATION:- SUCCESSFUL`. `scripts/cargo_dev.sh test -p rmath --lib validate_gnu_bytecode_stream` → exit 0.

### Step 4: Mutation check for the framing proof

Temporarily change one accepted width in a test-only copy, or negate `end > code.len()` inside a `#[cfg(kani_mutant)]` block that is not compiled by default. The point is to show the harness fails when the check is removed. Do not leave the mutant in the default `cfg(kani)` path.

If a local edit of the comparison makes the harness fail, restore the comparison in the same commit as the harness. Document the mutant command in a comment above the harness: the one-line edit, and that Kani reported a failure.

**Verify**: with the mutant applied, the framing harness exits non-zero. After restore, step 3's command succeeds again. `git diff` does not contain the mutant.

### Step 5: Prove vector-length admission

If `BinaryReader` can be constructed inside `serialize/core.rs` without a session, harness `read_vector_length` directly. Otherwise extract exactly one pure function and call it from `read_vector_length`:

```rust
fn vector_length_fits(
    len: i32,
    remaining: usize,
    element_bytes: usize,
    ascii: bool,
) -> Result<i32, String>
```

The production method must call it. Do not leave a second copy of the arithmetic.

Properties over symbolic `len: i32`, `remaining: usize` bounded to `0..=64`, and `element_bytes` in `{0, 1, 4, 8, 16}`:

- `len < 0` → `Err` containing `negative`.
- `element_bytes == 0` and `ascii == false` → `Err`, never a panic. This may require the small code change described in Current state.
- For `minimum >= 1`, `Ok(len)` iff `len as usize <= remaining / minimum`.
- `Ok` does not allocate. The pure function must not call the arena.
- `kani::cover` one accepted length and one truncated length.

**Verify**: `scripts/cargo_kani.sh -p rmath --no-default-features --harness vector_length_fits_rejects` → `VERIFICATION:- SUCCESSFUL`. Existing serialize tests still pass: `scripts/cargo_dev.sh test -p rmath --lib serialize::` → exit 0.

### Step 6: Document the lane

Add a short section to `scripts/cargo_kani.sh`'s header comment, not a new essay:

- verifier version `0.68.0`
- compiler `nightly-2026-08-21`
- the exact invocation
- "does not replace Miri, GC torture, fuzz, or `conformance_parity.sh`"
- unwinding assertions stay on

Do not add a pull-request CI job. Optional: a `workflow_dispatch` workflow is allowed if it only runs `scripts/cargo_kani.sh` for the two harness names and does not install a floating Kani.

**Verify**: `git diff -- rust-toolchain.toml Cargo.toml` shows no toolchain or default-feature change. `git diff --stat` lists only in-scope files.

## Test plan

- New Kani harnesses: framing partition, framing reject, vector-length fits, vector-length zero-width.
- New or extended unit test: width-table length, and zero element width returns `Err` if step 5 changes code.
- Mutant: framing comparison inverted, proof fails, mutant not committed.
- Pattern for ordinary tests: the existing `#[test]` modules in `bytecode.rs`.

## Done criteria

- [ ] `scripts/cargo_kani.sh -p rmath --no-default-features --harness validate_gnu_stream_partition` prints `VERIFICATION:- SUCCESSFUL`
- [ ] `scripts/cargo_kani.sh -p rmath --no-default-features --harness vector_length_fits_rejects` prints `VERIFICATION:- SUCCESSFUL`
- [ ] A recorded mutant of the framing check makes Kani fail, and that mutant is not in the diff
- [ ] `scripts/cargo_dev.sh test -p rmath --lib bytecode::` exits 0
- [ ] `scripts/cargo_dev.sh test -p rmath --lib serialize::` exits 0
- [ ] `rust-toolchain.toml` still contains `channel = "1.96.0"`
- [ ] `git status` shows no files outside the in-scope list
- [ ] `plans/README.md` status row for 001 is `DONE`

## STOP conditions

Stop and report back (do not improvise) if:

- The excerpts in Current state do not match the files.
- Kani 0.68.0 cannot be installed, or its setup wants a nightly other than `nightly-2026-08-21`.
- The crate fails to build on Kani's nightly for a reason other than the new harness (edition, dependency, `faer`). Do not upgrade the workspace toolchain to get past it.
- A harness needs `--unwinding-assertions` disabled, a stub of the collector, or `panic = abort` to succeed.
- `vector_length_fits` would change any `Ok` result for element widths 1, 4, 8, or 16.
- Making the script work seems to require editing `rust-toolchain.toml`. Use the detached `kani/` fallback or STOP.

## Maintenance notes

- Later proofs call `scripts/cargo_kani.sh` and add `--harness` names. They do not install a different Kani.
- `validate_gnu_adapter_impl` returning `Ok(false)` means "framed, but outside the bounded adapter." Do not fold that function into these harnesses.
- When GNU opcode widths change, update `GNU_BC_OPERAND_WIDTHS` and the pinned fixture `crates/rmath/tests/fixtures/gnu-bytecode-opcodes.tsv` together. The partition proof should then fail until the table and the walk agree.
- Reviewers should reject a green proof that forgot `kani::cover` on both an accepted and a rejected input.
