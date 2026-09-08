# wgpu 29.0.4 WebGPU callback compatibility

Source: the unmodified `wgpu` 29.0.4 crate from crates.io, except the four
callback constructors described below. Upstream: https://github.com/gfx-rs/wgpu.
Upstream MIT and Apache 2.0 licenses are retained alongside the source.

Rport builds Wasm with `panic=unwind` to implement R errors and control flow.
wasm-bindgen 0.2.127 requires `UnwindSafe` for the default JavaScript callback
constructors. wgpu's callbacks contain RefCell and erased user callbacks, so
wgpu 29.0.4 does not compile with this setting.

In src/backend/webgpu.rs, the three `Closure::once` calls use
`Closure::once_aborting`, and the one `Closure::wrap` uses
`Closure::wrap_aborting`. These are wasm-bindgen's explicit APIs for callbacks
that do not catch panics. A panic inside a GPU callback remains fatal; it must
not continue through possibly corrupted renderer state. These callbacks never
evaluate R. Ordinary R evaluation retains its unwind behavior and session
error handling. No changes were made to native GPU code or unsafe internals.

Remove this patch when an upstream release supports these callback constructors
with Rport's Wasm unwind configuration. See:
https://wasm-bindgen.github.io/wasm-bindgen/reference/passing-rust-closures-to-js.html
