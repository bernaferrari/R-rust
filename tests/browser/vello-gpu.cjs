// Optional GPU bundle: RPORT_WASM_FEATURES=vello-gpu scripts/build_wasm_runtime.sh --target web --out-dir /tmp/rport-gpu-pkg
// Run with RPORT_GPU_WASM_PKG=/tmp/rport-gpu-pkg node tests/browser/vello-gpu.cjs.
// Fails when WebGPU is unavailable: it must never silently test the CPU path.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');
const root = path.resolve(process.env.RPORT_GPU_WASM_PKG || '/tmp/rport-gpu-pkg');
const server = http.createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, 'http://localhost').pathname;
    if (pathname === '/') { res.setHeader('Content-Type', 'text/html'); res.end('<!doctype html><title>Vello GPU test</title>'); return; }
    const file = path.resolve(root, '.' + decodeURIComponent(pathname));
    if (!file.startsWith(root + path.sep)) { res.writeHead(403).end(); return; }
    const bytes = await fs.readFile(file);
    res.setHeader('Content-Type', file.endsWith('.wasm') ? 'application/wasm' : 'text/javascript');
    res.end(bytes);
  } catch { res.writeHead(404).end(); }
});
(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({ headless: true, args: ['--enable-unsafe-webgpu'] });
  try {
    const page = await browser.newPage();
    page.on("console", msg => console.log("browser:", msg.text()));
    page.on("pageerror", error => console.error("browser error:", error));
    const timeout = setTimeout(() => browser.close(), 120000);
    timeout.unref();
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const result = await page.evaluate(async () => {
      const mod = await import('/r_wasm.js');
      await mod.default();
      console.log("module initialized");
      const session = new mod.WasmRSession();
      const scene = session.record_scene("plot(1:3,3:1,col='red',main=expression(frac(alpha,beta)))", 240, 180);
      // Owned scene survives interpreter destruction before the first await.
      session.close(); session.free();
      console.log("scene recorded, requesting GPU");
      const gpu = await mod.WasmGpuRenderer.create();
      console.log("GPU ready", gpu.adapter_name());
      const pending = gpu.render_png(scene);
      let busyRejected = false;
      try { await gpu.render_png(scene); } catch (error) {
        busyRejected = /busy or closed/.test(String(error));
      }
      if (!busyRejected) throw new Error('concurrent GPU request did not reject');
      const first = await pending;
      console.log("first PNG rendered");
      const pendingSecond = gpu.render_png(scene);
      scene.free(); // Rendering owns its snapshot even if JavaScript frees this handle.
      const second = await pendingSecond;
      const image = await createImageBitmap(new Blob([first], {type: 'image/png'}));
      const canvas = new OffscreenCanvas(image.width, image.height);
      const ctx = canvas.getContext('2d'); ctx.drawImage(image, 0, 0);
      const pixels = ctx.getImageData(0, 0, image.width, image.height).data;
      let red = 0, ink = 0;
      for (let i = 0; i < pixels.length; i += 4) {
        if (pixels[i] > 150 && pixels[i+1] < 100 && pixels[i+2] < 100) red++;
        if (pixels[i] < 150 && pixels[i+1] < 150 && pixels[i+2] < 150) ink++;
      }
      const result = {width: image.width, height: image.height, red, ink,
        signature: Array.from(first.slice(0, 8)), repeatBytes: second.length,
        adapter: gpu.adapter_name()};
      image.close(); gpu.free(); return result;
    });
    assert.deepEqual(result.signature, [137,80,78,71,13,10,26,10]);
    assert.equal(result.width, 240); assert.equal(result.height, 180);
    assert.ok(result.red > 5, 'GPU rendered colored points');
    assert.ok(result.ink > 20, 'GPU rendered axes and mathematical label');
    assert.ok(result.repeatBytes > 100, 'renderer can be reused');
    clearTimeout(timeout);
    console.log('Vello GPU browser passed:', result);
  } finally { await browser.close(); server.close(); }
})().catch(error => { console.error(error); server.close(); process.exitCode = 1; });
