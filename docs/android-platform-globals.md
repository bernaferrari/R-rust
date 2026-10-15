# Android platform global policy

Android embedding must not accidentally depend on translated process globals.
The runtime still contains C-shaped fallback state while the port is being
sessionized, so every source file with mutable global patterns is classified in
`docs/android-platform-global-allowlist.tsv`.

This check verifies that declarations are inventoried. A passing inventory does
not approve `sessionize` or `platform-replace` entries as safe. The remaining
condition class filters, parser warnings, formatting and dispatch context,
device recursion flags, elapsed-time origin, translation directories and
process environment policy are tracked in `rport-eotc`. The inventory names
their actual declaration files after the module splits and distinguishes
test-only instrumentation from production state.

The checked policy is:

- `allow-session-dispatch`: compatibility state that exists to route legacy
  C-shaped calls into the active `RSession`.
- `allow-immutable`: immutable process sentinels or caches. These must not hold
  session-varying values.
- `allow-test`: test-only locks used to serialize global fallback tests.
- `allow-process`: intentional process resources, including panic-hook setup
  and collision-free temporary-name allocation across independent sessions.
- `sessionize`: real runtime state that must move into `RInstance` before the
  corresponding feature is Android-facing.
- `platform-replace`: platform state that must be provided by an Android host
  bridge or explicit session/device owner.
- `disabled-android`: desktop, fork, Tcl/Tk, X11, or process-startup code that
  is outside the Android embedding surface.

The scanner ignores declarations inside `#[cfg(test)] mod tests`,
`OnceLock<usize>` immutable sentinels, and the legacy `src/support/` mirror
tree. The compiled root modules (`src/graphapp`, `src/intl`, `src/tzone`) own
the Android-relevant state and are already wired through `RInstance`; the
support mirror is not part of the crate root module graph.

Standalone test files and `#[cfg(test)]` declarations outside an inline
`mod tests` remain visible to the conservative text scanner. Their inventory
entries explicitly record their test-only module or declaration scope.
The PPR settings and native LOESS prediction state now live in their original
runtime; they require no process-global classification.

Run the ratchet with:

```bash
scripts/check_android_globals.sh
```

`scripts/android_toolchain_check.sh` runs the same check before cross-compiling.
If a new mutable global appears, classify it in the TSV and either move it into
session state, gate it out of Android, or document why it is immutable/test-only.
