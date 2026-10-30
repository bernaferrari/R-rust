# Android platform global policy

Android embedding must not accidentally depend on translated process globals.
The runtime still contains C-shaped fallback state while the port is being
sessionized, so each discovered declaration must be classified in
`docs/android-platform-global-declarations.tsv`. The old file-level
`docs/android-platform-global-allowlist.tsv` is historical context only; it
does not approve individual declarations.

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

The syntax-based scanner ignores declarations under conditions requiring
`test` and the legacy `src/support/` mirror. It records each recognized
`static mut`, immutable and foreign statics, `thread_local!`, and `lazy_static!`.
Immutable statics require review even when their type is an alias or an opaque
wrapper; their name does not establish the absence of interior mutability. Conditions and enclosing
scopes are part of a declaration's identifier. The scanner checks all source
files conservatively, so separately included test-only files can still appear.
It does not resolve arbitrary macro expansions or the module graph; a passing inventory is not a complete semantic global-state audit.
Rust `include!` invocations and macro bodies that visibly declare globals or
contain an `include!` outside recognized macros are errors; included Rust code
may lie outside the scanned tree. Module path overrides, including those nested
in `cfg_attr`, are also rejected until the module graph can be verified.

The declaration-level manifest starts unclassified: existing file-level policy
entries must be reviewed for each declaration rather than automatically copied.
The release check will fail until that review is complete. The PPR settings and
native LOESS prediction state now live in their original runtime.

Run the ratchet with:

```bash
scripts/check_android_globals.sh
```

`scripts/android_toolchain_check.sh` runs the same check before cross-compiling.
If a new mutable global appears, classify that declaration in the TSV and either move it into
session state, gate it out of Android, or document why it is immutable/test-only.
