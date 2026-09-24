# Verification plans

Written 2026-09-23 against commit `16223ff7`. These plans turn a Kani recommendation into work this tree can actually execute. They do not claim that the Rust interpreter is equivalent to GNU R.

Execute in the order below. Each executor reads one plan file only, honors its STOP conditions, and updates that plan's status row. Do not push unless the operator asks.

## What changed relative to the recommendation

The recommendation is right about the objective: prove production memory, rooting, handles, indexing, and small evaluation-state machines in parallel with compatibility work. It is wrong about several next steps. The corrections below are from the current tree plus Kani 0.68.0's own docs.

| Recommendation | What this repo actually requires |
| --- | --- |
| Start Kani on the project toolchain (Rust 1.96.0) | Kani 0.68.0 compiles with its own pin, `nightly-2026-08-21`. Leave `rust-toolchain.toml` on 1.96.0. Kani is a separate lane, same pattern as Miri and the Wasm unwind nightly. |
| First proof is the whole root/handle/session stack | `RootTable::{claim,release,reprotect,restore}` is the proof. Guards, `ProtectScope`, `RInstance`, and embed `ValueHandle` paths pull thread-local session state and panic unwinding. Call the table methods directly. |
| Generation exhaustion is a clean `expect` | `next_gen` uses `checked_add(...).expect(...)`, but `claim` and `release` mutate the table before that call. A panic at `u64::MAX` leaves a new pointer under the old generation, or a null entry still matching the handle. Make the bump transactional before calling the proof green. |
| Checkpoint restore keeps "roots that should survive" | `ProtectScope` stores `root_table.checkpoint()` (`next_generation`) and calls `restore`, not `truncate`. Only `managed` slots survive. Raw `protect` roots are removed. `RootTable::truncate` is a different, currently unused operation. |
| Architecture doc is the spec | `docs/rust-r-port-architecture.md` still says the protect stack is a shifting LIFO `Vec` and that a generational table is roadmap. `protect.rs` already ships arbitrary-order generational slots. Proofs follow the code. Plan 002 corrects the paragraph. |
| Checked access, then bytecode execution, then argument matching | Pure leaves are faster than the root table and should land in the toolchain plan. GNU bytecode **framing** is already a pure function (`validate_gnu_bytecode_stream`); full execution is not. Argument matching is the first semantic proof and should not wait on the bytecode VM. There are two production matchers (`match_closure_args` and `matchArgs_NR_local`). |
| Prove `INTEGER_ELT` rejects bad indexes | GNU `INTEGER_ELT` is unchecked. This port matches that: bounds live in `checked_element_slot` and `Sexp::try_*_elt`. Adding checks to `INTEGER_ELT` would be a faithfulness change, not a proof. |
| Prove the live generational collector next | The marker is hard-wired to `SEXP` unions, `RInstance`, and the arena. Mark and update are duplicated `match` arms, not a visitor table. Extract a shared child function before any collector proof. Do not replace the non-moving collector. |
| Spend early budget on faer/LAPACK accuracy | Workspace admission is already `checked_mul` plus oracle tests. Kani does not prove numerical accuracy. Defer adapters. |
| One percentage of "formally verified R" | Record each harness with `cargo kani list`: domain, bounds, assumptions, toolchain, result. Miri, GC torture, fuzz, and the pinned GNU R oracle stay independent. |

## Execution order and status

| Plan | Title | Priority | Effort | Depends on | Status |
| --- | --- | --- | --- | --- | --- |
| 001 | Stand up a separate Kani lane and prove two production leaves | P1 | M | — | DONE |
| 002 | Make root-generation updates transactional and prove the production root table | P1 | L | 001 | DONE |
| 003 | Prove the checked string/list element decision on the code that ships | P1 | M | 001 | DONE |
| 004 | Specify argument matching once and check both production ports against it | P2 | L | 001 | DONE |

002 and 003 can run in parallel after 001. 004 can start after 001; it must not rewrite either matcher until the oracle shows they agree.

`argmatch_spec` is in the tree. A 2×2 symbolic run of that harness still builds a solver problem large enough to exhaust memory, so the machine-checked argument-match evidence that finished is the eleven-row oracle against both production matchers. The other harnesses below completed with `VERIFICATION:- SUCCESSFUL`.

## Dependency notes

- 001 is the only plan allowed to discover how `cargo kani` coexists with `rust-toolchain.toml`. Later plans consume that command unchanged.
- 002's proofs are invalid if they `assume` the generation counter away from `u64::MAX` without a separate harness for the boundary. The transactional bump is what makes the boundary a real property.
- 003 must not "fix" `INTEGER_ELT` / `REAL_ELT` / `LOGICAL_ELT`. Those are the unchecked GNU-shaped helpers.
- 004's pure matcher is a specification oracle. Replacing `match_closure_args` or `matchArgs_NR_local` is a later change, and only after both ports match the oracle on the cases listed in that plan.

## Allocation

Keep compatibility work on the majority of engineering time. Verification owns the four plans above, not a new collector, not faer, and not `RSession::eval`. Revisit the share only after a proof has caught a regression the existing tests missed, or after plan 002's invariant is being used to reject a caller change.

## Findings considered and rejected

- **Prove `RSession::eval`, `RCNTXT` jumps, or `tryCatch` with Kani.** R errors are `panic_any(RError)` caught by `catch_unwind` (`crates/rmath/src/sexp/context.rs`). Kani 0.68.0 does not model panic-stack unwinding. Keep Miri, fuzz (`fuzz/fuzz_targets/eval_boundary.rs`, `handle_boundary.rs`), and conformance for that boundary.
- **Prove embed `ValueHandle` define/read/write.** Those paths evaluate into `..rport_handles..`. The pure check is session id plus slot generation; the fuzz target already owns panic containment.
- **Add bounds checks to `INTEGER_ELT` so a harness goes green.** Unfaithful to GNU R. Prove `checked_element_slot` and `Sexp::try_*_elt` instead.
- **Kani the live `gengc` mark/sweep as milestone 5.** Blocked on a shared child-edge function. Existing evidence stays: worklist test (30,000 native / 256 under Miri), remembered-set regressions, `scripts/gc_torture_stress.sh`, nightly Miri. `gctorture` in this port full-marks and sweeps old nodes only; do not describe it as upstream's full young sweep.
- **Prove faer decompositions or LOESS numeric error.** Different obligation from memory safety. LOESS workspace admission and LAPACK contract tests already exist (`crates/rmath/src/library/stats/loess/mod.rs`, `crates/rmath/src/modules/lapack/`).
- **Inductive loop and function contracts on the first harness.** Kani has them (`-Z loop-contracts`, `-Z function-contracts`) and they are still experimental. Use them only after a bounded harness is green and its unwind assertions stay enabled. `#[kani::loop_decreases]` is optional total-correctness, not the default.
- **A second root-table implementation written for the prover.** Proofs call production `RootTable` methods.
- **Turning verification on by switching `panic = abort`.** Production Wasm and the session boundary require unwind. Proofs cover explicit `Result` or pure functions. Unwind is tested, not proved, until Kani supports it.
- **Replacing the non-moving collector, enabling ALTREP, or splitting `rmath` into new crates as part of this effort.** Closed decisions. ALTREP stays off (`scripts/check_altrep_disabled.sh`). A detached `kani/` package is allowed only if plan 001 cannot invoke Kani without editing the workspace toolchain pin.

## Later work, not scheduled here

1. Shared `pc` advance used by both `validate_gnu_bytecode_stream` and `eval_gnu_adapter`, then a proof that every GNU opcode arm advances by `GNU_BC_OPERAND_WIDTHS`. Do not prove the validator's stack-effect model equivalent to the executor: `Ok(false)` is an intentional "not in the bounded adapter" result.
2. One `children(...)` function used by both mark and update in `gengc.rs`, then a bounded mark proof on a tiny graph. Root census and write barriers stay tests.
3. Pure workspace-size helpers for QR/SVD/LOESS admission only, after the four plans above are green.
4. A verification ledger page generated from `cargo kani list`, with separate statuses: full machine domain, bounded, inductive contract, dynamic test, not established.

## Commands every executor inherits

| Purpose | Command | Expected |
| --- | --- | --- |
| Local test/clippy | `scripts/cargo_dev.sh test -p rmath <filter>` and `scripts/cargo_dev.sh clippy -p rmath --all-targets -- -D warnings` | exit 0 |
| Toolchain pin | `rust-toolchain.toml` stays `channel = "1.96.0"` | unchanged in `git diff` |
| Kani | the command recorded by plan 001 in `scripts/cargo_kani.sh` | that script's documented success line |
| Oracle behavior | `scripts/conformance_parity.sh --check --strict` when an R-visible result changes | strict parity holds |

`scripts/cargo_dev.sh` forwards arguments to `cargo` and then prunes stale binaries. It does not change compiler flags. Kani is not routed through it until plan 001 says otherwise.
