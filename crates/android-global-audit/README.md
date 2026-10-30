# Android global declaration inventory

Run `scripts/cargo_dev.sh run -p rport-android-global-audit -- . OUTPUT` from
the repository root. The output is a sorted, tab-separated inventory of path,
kind, and scoped declaration identifier. Duplicate identifiers, source parse
failures, Rust `include!` invocations, and visibly global declarations in other
macro bodies are errors. Unresolved Rust includes can insert code outside the
scanned tree and require review; `include_str!` and `include_bytes!` are data.
The scanner excludes conditions requiring `test` and the legacy `support`
directories; other configuration conditions are recorded rather than evaluated.

The Android release gate compares this inventory with the declaration-level
manifest. Classification is an inventory, not an approval of session isolation. It detects mutable
statics (including immutable and foreign declarations) and declarations inside
`thread_local!` and `lazy_static!`. All immutable statics need review because
type aliases and wrappers can hide interior mutability; their inventory kind is
not a claim that the value is safe. It does not expand arbitrary macros or follow module inclusion to determine
whether a file is compiled. A macro can generate state without spelling a
literal declaration in its body. Those limitations require further review before
the inventory can replace the Android release check.
