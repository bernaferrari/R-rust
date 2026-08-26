# Plan 006: Reconcile Android SAF projects without stale or unbounded mirrors

> **Executor instructions**: Follow each step and STOP condition. A reviewer
> maintains the plan index.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- rstudio-mobile/app/src/main/java/com/rstudio/mobile/data/ProjectRepository.kt rstudio-mobile/app/src/main/java/com/rstudio/mobile/runtime/RStudioRuntime.kt rstudio-mobile/app/src/test`

## Status

- **Priority**: P0
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: `plans/004-android-editor-integrity.md`
- **Category**: bug
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.2`

## Why this matters

The app executes R against a private mirror of an SAF folder, but refresh only
copies files into the mirror. External deletes/renames leave stale files that R
can still read, while imports copied into the mirror are never written back to
SAF. Recursive copying has only an imprecise 5,000-entry check and no depth,
per-file, or total-byte bound, allowing a selected folder to exhaust app
storage. Project behavior must be one coherent, bounded contract.

## Current state

- `ProjectRepository.kt:118-127` imports into `project.localRoot` only.
- `ProjectRepository.kt:154-168` reuses the existing mirror and scans on top of
  it; it never removes stale paths or swaps an atomic staging directory.
- `ProjectRepository.kt:170-200` recursively copies every file. The entry limit
  is checked once per directory, so siblings can exceed it; there are no depth
  or byte limits.
- Rename/delete at lines 99-107 affect SAF only; runtime refresh is launched
  asynchronously at `RStudioRuntime.kt:529-557`, leaving completion/status
  ordering nondeterministic.

## Commands

| Purpose | Command | Expected |
|---|---|---|
| Repository unit tests | `ANDROID_HOME="$HOME/Library/Android/sdk" ./gradlew :app:testDebugUnitTest --no-daemon` | exit 0 |
| Android compile | `ANDROID_HOME="$HOME/Library/Android/sdk" ./gradlew :app:compileDebugKotlin --no-daemon` | exit 0 |
| Lint | `ANDROID_HOME="$HOME/Library/Android/sdk" ./gradlew :app:lintDebug --no-daemon` | exit 0 |

## Scope

**In scope**:

- `rstudio-mobile/app/src/main/java/com/rstudio/mobile/data/ProjectRepository.kt`
- Project-related methods in `.../runtime/RStudioRuntime.kt`
- New repository/runtime JVM tests and minimal fake storage abstractions needed
  to test reconciliation without a device.

**Out of scope**:

- Cloud sync, Git, multi-user conflict resolution, or arbitrary binary editing.
- Deleting any real user tree during tests.
- Silently choosing whether mirror-only imported files should persist; encode
  and document one explicit policy.

## Git workflow

- Branch `advisor/006-saf-reconciliation`, isolated worktree.
- Conventional commit: `fix(android): reconcile project storage safely`.
- Do not push or modify the user's branch.

## Steps

### Step 1: Define and test the project storage contract

Treat SAF as source of truth for project-visible files. Imports into an active
project must create/copy a document in SAF and then mirror it. Runtime-created
files must be synchronized deliberately or classified as transient in a
separate ignored directory. Put filesystem-independent reconciliation planning
behind a small testable abstraction.

**Verify**: contract tests cover add, modify, delete, rename, import, and a
transient runtime artifact.

### Step 2: Rebuild via bounded staging and atomic swap

Scan into a sibling staging directory, enforcing the limit before every entry,
maximum depth, safe per-file bytes, and safe aggregate bytes while streaming.
On success replace the old mirror; on failure preserve the previous complete
mirror and delete only staging. Reject path traversal after sanitization.

Use named constants with user-facing errors. Choose generous mobile defaults
and document them; do not silently truncate.

**Verify**: tests prove stale files disappear after successful refresh; failed,
oversized, overdeep, and overcount scans retain the old mirror.

### Step 3: Make mutations await reconciliation

Rename/delete/create/import operations must await repository completion and a
single refresh before setting final Ready/status state. Update any open
document metadata by stable URI through Plan 004 helpers. Surface persistable
permission failure rather than swallowing it when future restoration depends
on the grant.

**Verify**: coroutine tests assert no nested launch/race and correct status/file
state after each mutation.

### Step 4: Run the Android gate

Run unit tests, compile, and lint. Inspect scope and ensure tests use only temp
directories/fakes.

## Done criteria

- [ ] A successful refresh exactly mirrors SAF-visible project files.
- [ ] Project imports are visible in SAF and survive restart.
- [ ] Entry/depth/per-file/total-byte limits are exact, streaming, and tested.
- [ ] Failed refresh preserves the prior mirror; stale paths disappear only
  after a complete successful staging scan.
- [ ] Mutations await one reconciliation and keep open-document identity valid.
- [ ] Unit tests, compile, and lint pass; only in-scope files change.

## STOP conditions

Stop if atomic replacement cannot be implemented without risking the only good
mirror, Android's SAF provider cannot support project import for the selected
tree, the correct source-of-truth policy conflicts with documented user data,
or verification fails twice.

## Maintenance notes

Resource limits are product policy and should be configurable/tested, not magic
copy-loop details. Future runtime outputs should go through an explicit project
sync API rather than writing directly into the mirror.

