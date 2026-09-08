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
Native window lifecycle remains host-owned; the wasm binding provides an
explicit browser canvas attach/resize/present path described below.

Run actual GPU tests (they fail if an adapter is unavailable):

```sh
cargo test -p r-device-vello-gpu -- --include-ignored --nocapture
cargo test -p r-embed --features vello-gpu --test graphics_gpu -- --ignored --nocapture
```

The optional `r-wasm/vello-gpu` API uses `WasmRSession.record_scene`,
`await WasmGpuRenderer.create()` and `await gpu.render_png(scene)`. R errors retain
Wasm unwinding; a narrow vendored wgpu callback patch is documented in
[`vendor/wgpu/RPORT-PATCH.md`](../../vendor/wgpu/RPORT-PATCH.md).

For direct browser presentation, attach a caller-owned canvas after creating
the renderer, then render owned scenes directly into its WebGPU swapchain:

```js
const gpu = await WasmGpuRenderer.create();
gpu.attach_canvas(canvas, canvas.width, canvas.height);
await gpu.render_canvas(scene);
gpu.resize_canvas(nextWidth, nextHeight);
await gpu.render_canvas(resizedScene);
```

`attach_canvas` configures a real canvas surface and `render_canvas` presents
the acquired frame; neither operation performs PNG or RGBA readback. Vello
renders into its required `Rgba8Unorm` storage texture and blits that texture
into the surface, which also handles browsers that expose BGRA swapchains. The
host owns the canvas and must resize its backing pixels before `resize_canvas`;
zero dimensions and scene/canvas size mismatches are rejected. Lost or
outdated surfaces are reported as errors so the host can recreate the surface.

Native hosts may use `render_texture` for their own compositor or the safe
surface API below for direct presentation. This crate intentionally does not
accept raw Android `ANativeWindow` or Metal pointers across UniFFI: surface
creation and lifecycle remain in the native host.

Rust native hosts that already own a wgpu-compatible window can use the safe
surface API directly:

```rust
let (mut gpu, mut surface) = r_device_vello_gpu::GpuRenderer::new_for_surface(
    window, 640, 480,
).await?;
gpu.render_surface(&surface, &scene).await?;
gpu.resize_surface(&mut surface, 800, 600)?;
```

`window` is converted through wgpu's `SurfaceTarget` and stays alive for the
returned surface lifetime. No raw platform pointer or UniFFI texture handle is
accepted. Android and Swift UI host integration still needs a host-specific
window ownership adapter.

```sh
RPORT_WASM_FEATURES=vello-gpu scripts/build_wasm_runtime.sh --target web --out-dir /tmp/rport-gpu-pkg
RPORT_GPU_WASM_PKG=/tmp/rport-gpu-pkg node tests/browser/vello-gpu.cjs
```

The browser test requires Playwright and real WebGPU availability. Set
`RPORT_GPU_BROWSER_CHANNEL=chrome` (the default in this test) to use an
installed Chrome channel. Playwright's bundled Chromium may expose WebGPU but
currently returns invalid canvas surface textures in this environment; that is
an infrastructure/backend limitation, and the test keeps the channel
override explicit. The standard production browser/mobile bundle keeps its
CPU backend by default.
