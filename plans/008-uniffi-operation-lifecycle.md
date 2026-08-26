# Plan 008: Repair UniFFI operation lifecycle and cancellation

> **Executor instructions**: Implement one explicit state machine, run every
> race/lifecycle test, and stop rather than preserve ambiguous callbacks.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- crates/r-uniffi/src/lib.rs rstudio-mobile/app/src/main/java/com/rstudio/mobile/runtime/RStudioRuntime.kt rstudio-mobile/app/generated`

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: Plans 001 and 003
- **Category**: bug / tech-debt
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.6`

## Why this matters

All commands share one resettable cancellation token, callbacks have no
operation identity, `Running` is never assigned, terminal states accumulate,
and async render throws away its plot. Constructor success precedes worker
initialization and Drop never joins the worker. Android's one global Boolean can
therefore treat a stale completion as the current run, especially after
cancel→rerun, while long sessions leak state/threads.

## Current state

- `crates/r-uniffi/src/lib.rs:341-348` callback methods carry payloads but no
  operation ID.
- `:405-412` stores one `CancellationToken` and an unbounded status map.
- `:441-449` spawns a detached worker and `expect`s session initialization.
- `:613-618,836-839` sends shutdown but retains/joins no `JoinHandle`.
- `:667-670,725-758` resets the shared token; a newer request can clear an
  earlier cancellation. A waiter thread is spawned per async operation.
- `:773-813` reports render as a fabricated EvalResult and discards the plot.
- `RStudioRuntime.kt:230-250,279-290,839-843` correlates every callback with one
  `AtomicBoolean` and marks cancellation complete immediately.

The current tests at `r-uniffi/src/lib.rs:889-980` provide a callback recorder;
extend that pattern with IDs and deterministic barriers/condvars rather than
sleep-only races.

## Commands

| Purpose | Command | Expected |
|---|---|---|
| UniFFI tests | `cargo test -p r-uniffi -- --test-threads=1` | exit 0 |
| Bindings | `scripts/generate_uniffi_bindings.sh --check` | exit 0 and no diff |
| Android unit | `cd rstudio-mobile && ANDROID_HOME="$HOME/Library/Android/sdk" ./gradlew :app:testDebugUnitTest --no-daemon` | exit 0 |
| Format/lint | `cargo fmt --check --all && cargo clippy -p r-uniffi --all-targets --all-features -- -D warnings` | exit 0 |

## Scope

**In scope**:

- `crates/r-uniffi/src/lib.rs`
- Generated/checked Kotlin binding only through the repository generator
- Android runtime callback/task state plus new pure state tests
- Shared contract types only if operation identity belongs in the common
  platform-neutral API

**Out of scope**:

- Parser/GC changes, WebR internals, UI redesign, or hard-kill process isolation.
- Retaining an unbounded operation history for compatibility.

## Git workflow

- Branch `advisor/008-uniffi-lifecycle`, isolated worktree.
- Conventional commit: `fix(uniffi): scope operation lifecycle and cancellation`.
- Do not push or modify the user's branch.

## Steps

### Step 1: Define typed, operation-scoped state

Give every queued eval/render an ID and cancellation token. Carry the ID through
commands, callbacks, progress/output/error, and terminal status. Model
`Pending -> Running -> Completed/Failed/Cancelled`; represent eval and plot
results without fabricating one as the other.

Expose consume/take semantics or a bounded retention policy. A stale ID must
never change another operation's state.

**Verify**: unit tests assert legal transitions, typed render retrieval, and
bounded terminal storage.

### Step 2: Remove reset races and waiter-thread fanout

The worker should own transition and completion updates directly. Cancelling ID
A must not be reset by enqueuing B; define whether B waits or is rejected until
A acknowledges cancellation. Do not spawn one waiter thread per operation.

**Verify**: barrier-controlled cancel A → request B tests prove A cannot
complete/publish as B and B eventually returns the expected result.

### Step 3: Handshake startup and join shutdown

Have constructor wait for a readiness/error result from `RSession::new` before
returning success. Store the worker handle, make close idempotent, signal
cancellation/shutdown, and join outside held locks. Drop must not leak or
deadlock when an operation is active.

**Verify**: injected init failure returns `InitFailed`; repeated close is safe;
drop-during-eval completes within a bounded test timeout and worker count/state
returns to baseline.

### Step 4: Make Android consume operation identity

Replace `AtomicBoolean` with `Idle/Running(id)/Cancelling(id)`. Ignore stale
callbacks, remain non-runnable while cancellation is pending, and enable the
next run only after terminal acknowledgement. Regenerate bindings through the
official script.

**Verify**: pure Android state tests cover normal completion, stale callback,
cancel acknowledgement, cancel→rerun, error, and reset.

## Done criteria

- [ ] Tokens, callbacks, results, and state transitions are operation-scoped.
- [ ] Async plot returns the actual plot; terminal state is consumable/bounded.
- [ ] Startup reports init failure synchronously and shutdown joins safely.
- [ ] Android cannot misattribute stale callbacks and does not declare cancel
  complete prematurely.
- [ ] Rust/Android tests, binding freshness, format, and strict Clippy pass.

## STOP conditions

Stop if changing callback signatures cannot be represented by UniFFI 0.30,
shutdown needs blocking while holding a callback/session lock, compatibility
requires unbounded retention, or verification fails twice.

## Maintenance notes

Operation IDs are protocol identity, not UI decoration. Every future async
surface (help, packages, data export) should use the same state machine rather
than add a new Boolean or detached waiter.

