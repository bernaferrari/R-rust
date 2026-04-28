# Android platform global policy

Android embedding must not accidentally depend on translated process globals.
The runtime still contains C-shaped fallback state while the port is being
sessionized, so every source file with mutable global patterns is classified in
`docs/android-platform-global-allowlist.tsv`.

The checked policy is:

- `allow-session-dispatch`: compatibility state that exists to route legacy
  C-shaped calls into the active `RSession`.
- `allow-immutable`: immutable process sentinels or caches. These must not hold
  session-varying values.
- `allow-test`: test-only locks used to serialize global fallback tests.
- `sessionize`: real runtime state that must move into `RInstance` before the
  corresponding feature is Android-facing.
- `platform-replace`: platform state that must be provided by an Android host
  bridge or explicit session/device owner.
- `disabled-android`: desktop, fork, Tcl/Tk, X11, or process-startup code that
  is outside the Android embedding surface.

The scanner ignores declarations inside `#[cfg(test)] mod tests` and
`OnceLock<usize>` immutable sentinels; neither is Android runtime state.

Run the ratchet with:

```bash
scripts/check_android_globals.sh
```

`scripts/android_toolchain_check.sh` runs the same check before cross-compiling.
If a new mutable global appears, classify it in the TSV and either move it into
session state, gate it out of Android, or document why it is immutable/test-only.
