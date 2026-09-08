// Optional GPU bundle: RPORT_WASM_FEATURES=vello-gpu scripts/build_wasm_runtime.sh --target web --out-dir /tmp/rport-gpu-pkg
// Run with RPORT_GPU_WASM_PKG=/tmp/rport-gpu-pkg node tests/browser/vello-gpu.cjs.
// Fails when WebGPU is unavailable: it must never silently test the CPU path.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');
const { PNG } = require('pngjs');
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
    const browser = await chromium.launch({ channel: process.env.RPORT_GPU_BROWSER_CHANNEL || 'chrome', headless: true, args: ['--enable-unsafe-webgpu'] });
  try {
    const page = await browser.newPage();
    page.on("console", msg => console.log("browser:", msg.text()));
    page.on("pageerror", error => console.error("browser error:", error));
    const timeout = setTimeout(() => browser.close(), 120000);
    timeout.unref();
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const jsCanvasProbe = await page.evaluate(async () => {
      if (!navigator.gpu) return { supported: false };
      const canvas = document.createElement('canvas');
      canvas.width = 64; canvas.height = 64; document.body.append(canvas);
      const adapter = await navigator.gpu.requestAdapter();
      if (!adapter) return { supported: false, reason: 'no adapter' };
      const device = await adapter.requestDevice();
      const context = canvas.getContext('webgpu');
      const format = navigator.gpu.getPreferredCanvasFormat();
      context.configure({ device, format, alphaMode: 'premultiplied' });
      const texture = context.getCurrentTexture();
      const encoder = device.createCommandEncoder();
      const pass = encoder.beginRenderPass({ colorAttachments: [{
        view: texture.createView(), clearValue: { r: 0.1, g: 0.2, b: 0.9, a: 1 }, loadOp: 'clear', storeOp: 'store'
      }] });
      pass.end(); device.queue.submit([encoder.finish()]);
      await new Promise(requestAnimationFrame);
      return { supported: true, format, width: canvas.width, height: canvas.height };
    });
    console.log('pure JS WebGPU canvas probe:', jsCanvasProbe);
    const result = await page.evaluate(async () => {
      const mod = await import('/r_wasm.js');
      await mod.default();
      console.log("module initialized");
      const session = new mod.WasmRSession();
      const scene = session.record_scene("plot(1:3,3:1,col='red',main=expression(frac(alpha,beta)))", 240, 180);
      const largeScene = session.record_scene("plot(1:3,3:1,col='blue')", 320, 240);
      // Owned scene survives interpreter destruction before the first await.
      session.close(); session.free();
      console.log("scene recorded, requesting GPU");
      const displayCanvas = document.createElement('canvas');
      document.body.append(displayCanvas);
      const gpu = await mod.WasmGpuRenderer.create_for_canvas(displayCanvas, 240, 180);
      console.log("GPU ready", gpu.adapter_name());
      await gpu.render_canvas(scene);
      await new Promise(requestAnimationFrame);
      gpu.resize_canvas(320, 240);
      await new Promise(requestAnimationFrame);
      await gpu.render_canvas(largeScene);
      await new Promise(requestAnimationFrame);
      console.log("direct canvas presentation passed", displayCanvas.width, displayCanvas.height);
      const screenshot = await createImageBitmap(await (await fetch(displayCanvas.toDataURL())).blob());
      const probe = new OffscreenCanvas(screenshot.width, screenshot.height);
      const probeContext = probe.getContext('2d');
      probeContext.drawImage(screenshot, 0, 0);
      const presentedPixels = probeContext.getImageData(0, 0, screenshot.width, screenshot.height).data;
      let presentedInk = 0;
      let presentedLight = 0;
      for (let i = 0; i < presentedPixels.length; i += 4) {
        if (presentedPixels[i] < 150 || presentedPixels[i + 1] < 150 || presentedPixels[i + 2] < 150) presentedInk++;
        if (presentedPixels[i] + presentedPixels[i + 1] + presentedPixels[i + 2] > 30) presentedLight++;
      }
      screenshot.close();
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
      largeScene.free();
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
      const rect = displayCanvas.getBoundingClientRect();
      const result = {width: image.width, height: image.height, red, ink, presentedInk, presentedLight,
        canvasRect: {x: rect.x, y: rect.y, width: rect.width, height: rect.height},
        signature: Array.from(first.slice(0, 8)), repeatBytes: second.length,
        adapter: gpu.adapter_name()};
      image.close(); gpu.free(); return result;
    });
    const screenshot = PNG.sync.read(await page.screenshot({ type: 'png', clip: result.canvasRect }));
    let screenshotInk = 0;
    let screenshotBlue = 0;
    for (let i = 0; i < screenshot.data.length; i += 4) {
      if (screenshot.data[i] + screenshot.data[i + 1] + screenshot.data[i + 2] > 30) screenshotInk++;
      if (screenshot.data[i + 2] > 100 && screenshot.data[i + 2] > screenshot.data[i] * 1.3) screenshotBlue++;
    }
    assert.ok(screenshotInk > 20, `direct canvas screenshot was blank (${screenshotInk} lit pixels)`);
    assert.ok(screenshotBlue > 5, `direct canvas screenshot has no blue plot ink (${screenshotBlue} pixels)`);
    assert.deepEqual(result.signature, [137,80,78,71,13,10,26,10]);
    assert.equal(result.width, 240); assert.equal(result.height, 180);
    assert.ok(result.red > 5, 'GPU rendered colored points');
    assert.ok(result.ink > 20, 'GPU rendered axes and mathematical label');
    assert.ok(result.repeatBytes > 100, 'renderer can be reused');
    clearTimeout(timeout);
    console.log('Vello GPU browser passed:', result);
  } finally { await browser.close(); server.close(); }
})().catch(error => { console.error(error); server.close(); process.exitCode = 1; });
