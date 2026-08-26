# Plan 004: Make Android editor identity, recovery, and saves race-safe

> **Executor instructions**: Execute the plan exactly, test each state
> transition, and stop on a STOP condition. A reviewer maintains the index.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- rstudio-mobile/app/src/main/java/com/rstudio/mobile/runtime/RStudioRuntime.kt rstudio-mobile/app/src/test`

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.2`

## Why this matters

Opening a project file updates top-level editor metadata without adding or
activating the corresponding document, so later typing can mutate one tab while
saving another path. Recovery replaces every persisted tab with one synthetic
document, and asynchronous saves mark whichever state is current as clean even
if the user typed or changed tabs during I/O. These are credible data-loss
paths in the app's primary workflow.

## Current state

- `RStudioRuntime.kt:139-160` lets a single recovery file replace the entire
  persisted document list. It also accepts a persisted `activeId` that may not
  exist and falls back only for displayed fields, leaving identity invalid.
- `RStudioRuntime.kt:347-365` and `:466-481` correctly upsert and activate
  normal/recent scripts. This is the convention project-open should match.
- `RStudioRuntime.kt:560-581` opens a project file but does not update
  `documents` or `activeDocumentId`.
- `RStudioRuntime.kt:370-440` captures some initial state for local save but
  later calls `.withActiveDocument(false)` against current state; `saveScriptTo`
  reads `_state.value.code` inside I/O and always marks current state clean.
- Existing app JVM tests cover only editor-selection helpers and report
  rendering; no document-state reducer is tested.

## Commands

| Purpose | Command | Expected |
|---|---|---|
| Unit tests | `ANDROID_HOME="$HOME/Library/Android/sdk" ./gradlew :app:testDebugUnitTest --no-daemon` | exit 0 |
| Shared tests | `./gradlew :shared:allTests --no-daemon` | exit 0 |
| Android compile | `ANDROID_HOME="$HOME/Library/Android/sdk" ./gradlew :app:compileDebugKotlin --no-daemon` | exit 0 |

If the SDK is elsewhere, use an already installed explicit path; do not create
or commit `local.properties`.

## Scope

**In scope**:

- `rstudio-mobile/app/src/main/java/com/rstudio/mobile/runtime/RStudioRuntime.kt`
- One new pure Kotlin state-transition file under the same runtime package if
  needed to make logic independently testable.
- New JVM tests under `rstudio-mobile/app/src/test/java/com/rstudio/mobile/runtime/`.

**Out of scope**:

- SAF mirroring/import behavior (Plan 006).
- UI redesign, Rust/UniFFI changes, or session concurrency.
- Dropping any persisted document or changing public picker behavior.

## Git workflow

- Branch `advisor/004-android-editor-integrity`, isolated worktree.
- Conventional commit: `fix(android): preserve document identity across I/O`.
- Do not push or modify the user's branch.

## Steps

### Step 1: Extract deterministic editor transitions

Introduce the smallest internal/pure helpers needed to apply document open,
recovery merge, active-id validation, and save completion to immutable
`RStudioUiState`. Reuse `EditorDocument` and the existing `upsert` convention;
do not create a parallel state model.

**Verify**: new JVM tests can call these helpers without constructing
`Application`, `RSession`, or Android content resolvers.

### Step 2: Preserve tabs and validate recovery identity

Restore persisted documents first. If `activeId` is missing, select the first
restored document and keep top-level fields consistent. Merge a recovery draft
into the matching URI/document where possible; otherwise add one recovery
document. Never discard unrelated tabs. Make the recovered document active and
dirty.

**Verify**: tests cover invalid active ID, matching recovery, unmatched
recovery, and preservation of two unrelated tabs.

### Step 3: Make every open path identity-consistent

Update `openProjectFile` to create/upsert an `EditorDocument`, set its ID using
the stable URI/path convention already used by other open paths, activate it,
and keep all top-level metadata synchronized. Add a regression proving edits
after opening update that exact document.

**Verify**: a state test opens project B while tab A is active, edits, and
asserts A is unchanged and B is dirty.

### Step 4: Version save completions

Capture target document ID, code snapshot, and destination metadata before I/O.
On completion, mark that document clean only if its code still equals the saved
snapshot. If the user typed during I/O, retain dirty=true. If another tab is now
active, do not overwrite its code/name/path/dirty state. Apply this to local,
project, and picker save paths through one helper.

**Verify**: tests cover edit-during-save, tab-switch-during-save, unchanged
save, and save-as metadata for the target document.

### Step 5: Run the Android JVM gate

Run all commands in the table and inspect `git status` for scope.

## Done criteria

- [ ] Project-open creates/activates the correct document.
- [ ] Recovery preserves all unrelated persisted documents and a valid active ID.
- [ ] Save completion never clears edits newer than its snapshot or mutates a
  different active tab.
- [ ] At least eight focused state-transition regressions pass.
- [ ] Android unit/shared tests and compile pass; only in-scope files change.

## STOP conditions

Stop if testing requires a real native `RSession`, the fix requires a storage
schema migration that can drop documents, a save cannot be associated with a
stable target ID, or verification fails twice.

## Maintenance notes

All future async editor I/O must carry target ID plus content/version snapshot.
Reviewers should reject any completion handler that calls
`withActiveDocument(false)` on unconstrained current state.

