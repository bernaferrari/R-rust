# Vello GPU device

This crate renders owned `r_graphics_engine::Scene` values with Vello's compute
pipelines through wgpu. It contains no unsafe Rust. Vello/wgpu and platform drivers
remain dependencies with their own unsafe implementations.

Enable `r-embed`'s `vello-gpu` feature for this embedding API:

```rust
let mut session = r_embed::RSession::new()?;
let scene = session.record_scene(
    "plot(1:10, main=expression(frac(alpha[1],sqrt(beta))))", 640, 480)?;
drop(session); // the scene owns every command, string and raster
let mut gpu = r_embed::GpuRenderer::new().await?;
let png = gpu.render_png(&scene).await?;
```

Keep `GpuRenderer` for repeated plots rather than recompiling pipelines each time.
`render_texture` returns a premultiplied RGBA8 texture on `gpu.device()` for host
compositing without pixel readback. `render_rgba` and `render_png` return straight
alpha pixels. Native readback blocks for up to 30 seconds; browser readback yields
to the event loop. Adapter initialization and rendering errors are explicit.
The caller can choose the separate CPU backend when GPU availability is optional.

Requires a compute-capable Metal, Vulkan, DX12 or WebGPU adapter. Canvas limits are
16,777,216 pixels and the device texture limit. Scene inputs are validated before
encoding. Per-path antialias disable is currently ignored by Vello GPU (MSAA16);
the legacy flag-incomplete ArcTo command uses the documented line fallback.
There is no automatic window/surface lifecycle or zero-copy browser canvas API.

Run actual GPU tests (they fail if an adapter is unavailable):

```sh
cargo test -p r-device-vello-gpu -- --include-ignored --nocapture
cargo test -p r-embed --features vello-gpu --test graphics_gpu -- --ignored --nocapture
```

The optional `r-wasm/vello-gpu` API uses `WasmRSession.record_scene`,
`await WasmGpuRenderer.create()` and `await gpu.render_png(scene)`. R errors retain
Wasm unwinding; a narrow vendored wgpu callback patch is documented in
[`vendor/wgpu/RPORT-PATCH.md`](../../vendor/wgpu/RPORT-PATCH.md).

```sh
RPORT_WASM_FEATURES=vello-gpu scripts/build_wasm_runtime.sh --target web --out-dir /tmp/rport-gpu-pkg
RPORT_GPU_WASM_PKG=/tmp/rport-gpu-pkg node tests/browser/vello-gpu.cjs
```

The browser test requires Playwright/Chromium and real WebGPU availability. The
standard production browser/mobile bundle keeps its CPU backend by default.
