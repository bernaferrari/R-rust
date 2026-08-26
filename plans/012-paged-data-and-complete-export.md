# Plan 012: Page and export data without whole-object transfer

> **Executor instructions**: Work from the reviewed Android/storage chain in an
> isolated worktree. Keep the R expression protocol narrow and fully tested.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: Plans 004 and 006
- **Category**: correctness / performance
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.2.4`

## Why this matters

The UI says it pages, but `inspectEnvironment` and every `loadMoreData` call
evaluate and serialize the entire data frame through UniFFI, then discard all
but 100 rows in Kotlin. CSV export writes only `DataTable.rows`, so a 10,000-row
object silently exports the currently loaded prefix. A data viewer cannot be
trusted while paging is cosmetic and export is incomplete.

## Current state

- `RStudioRuntime.kt:665-690` calls `get(name)` for initial and subsequent
  pages; `RValue.toDataTable` slices only after the full typed value crosses FFI.
- `RStudioRuntime.kt:707-724` writes only the in-memory page rows.
- `DataTableView.kt` correctly labels filtering as applying to loaded rows, but
  the export action does not warn that it is partial.

## Scope

**In scope**:

- Data inspection/paging/export paths in `runtime/RStudioRuntime.kt`
- A small pure data-page protocol/helper file and JVM tests
- `DataTableView.kt` only for truthful loading/error/empty-page UI
- Rust/UniFFI API files only if an app-side page envelope cannot avoid the full
  transfer without parsing printed output

**Out of scope**: spreadsheet editing, remote databases, arbitrary SQL, and
loading a full frame merely to infer page metadata.

## Steps

### Step 1: Return a typed page envelope from R

Evaluate a `local` expression which retrieves the named object, validates it is
a data frame or matrix, computes total rows, and subsets only `[offset + 1,
min(total, offset + limit), , drop = FALSE]`. Return a named list containing a
scalar total and the page. Parse that typed `RValue` envelope without printed
text. Escape the object name with the existing R-string helper and clamp offset
and limit before interpolation.

The initial inspector must determine pageability without first evaluating the
whole object into UniFFI. Non-tabular objects may continue through the normal
inspection path. Subsequent pages transfer at most `DATA_PAGE_SIZE` rows.

### Step 2: Give requests stable source and offset semantics

Capture source name and requested offset before I/O. Drop a late response when
the user selected a different object. Deduplicate/reject an already-running
request for the same next offset. Append only a page whose `rowOffset` equals
the expected contiguous offset; never duplicate rows after repeated taps.

### Step 3: Export the complete source object inside R

For a table backed by `dataSourceName`, ask R to `write.csv(get(name,
envir=.GlobalEnv), tempFile, row.names = FALSE)`, verify the evaluation result,
then stream that complete file to the selected URI and remove the cache file in
`finally`. Never reconstruct CSV from loaded display strings. For a table with
no live source, disable export or explicitly label it as a page export; do not
silently imply completeness.

### Step 4: Test bounded transfer and complete output

Add expression-builder/envelope unit tests and an integration test with more
than two page sizes. Assert page 1/page 2 boundaries, total count, stale response
rejection, no duplicates, quotes/NA in full CSV, and exported row count equal to
the source total while only one page is loaded.

## Verification

```sh
ANDROID_HOME=/Users/bernardoferrari/Library/Android/sdk ./gradlew :app:testDebugUnitTest --no-daemon
ANDROID_HOME=/Users/bernardoferrari/Library/Android/sdk ./gradlew :app:compileDebugKotlin --no-daemon
ANDROID_HOME=/Users/bernardoferrari/Library/Android/sdk ./gradlew :app:lintDebug --no-daemon
./gradlew :shared:allTests --no-daemon
```

## Done criteria

- [ ] A page request serializes no more than the requested page plus metadata.
- [ ] Late/repeated requests cannot mix objects or duplicate rows.
- [ ] Full export row count/content comes from the live R source, not loaded UI
      rows.
- [ ] Tests include a frame larger than two pages and CSV edge values.
- [ ] Android unit/compile/lint and shared tests pass.

## STOP conditions

Stop if the evaluator cannot subset a frame without materializing an FFI value,
if `write.csv` does not report failures reliably, if a live source cannot be
identified, or if complete export would require unbounded Kotlin memory.
