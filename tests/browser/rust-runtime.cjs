// Run against the built distribution; includes the real module Worker and Wasm.
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');
const root = path.resolve(__dirname, '../../rstudio-mobile/webApp/build/dist/wasmJs/productionExecutable');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css' };
const server = http.createServer(async (req, res) => {
  try {
    const requested = decodeURIComponent(new URL(req.url, 'http://localhost').pathname);
    const file = path.resolve(root, '.' + (requested === '/' ? '/index.html' : requested));
    if (!file.startsWith(root + path.sep)) { res.writeHead(403).end(); return; }
    const bytes = await fs.readFile(file);
    res.writeHead(200, {'Content-Type': types[path.extname(file)] || 'application/octet-stream'}).end(bytes);
  } catch (_) { res.writeHead(404).end(); }
});
(async () => {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({headless:true});
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', e => errors.push(String(e)));
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.waitForSelector('#console-command');
    assert.match(await page.locator('.runtime').innerText(), /Rport Rust runtime/);
    const request = (operation, code) => page.evaluate(({operation,code}) => rportRust.request(operation,code), {operation,code});
    assert.equal(await request('eval','x <- 41; x + 1'), '[1] 42');
    assert.equal(await request('string',"paste('a','b',sep='|')"),'a|b');
    await assert.rejects(request('eval',"stop('expected-error')"), /expected-error/);
    assert.equal(await request('eval','x'), '[1] 41', 'R errors preserve the session');
    assert.equal(await request('eval',"f <- function() { on.exit(cat('exit')); return(7L) }; f()"), 'exit\n[1] 7');
    await request('eval','i <- 0L; repeat { i <- i + 1L; if (i < 3) next; break }; stopifnot(i == 3)');
    assert.match(await request('plot',"plot(x=1:3,y=3:1,col='red')"), /data:image\/png;base64,/);
    await page.locator('#console-command').fill('x + 2');
    await page.locator('#console-run').click();
    await page.waitForFunction(() => document.querySelector('#console').textContent.includes('[1] 43'));
    assert.equal(await page.locator('#install').isDisabled(), true);
    const stopped = await page.evaluate(async () => {
      const result = rportRust.request('eval','repeat {}').catch(e => String(e));
      setTimeout(() => rportRust.cancel(), 100);
      return await result;
    });
    assert.match(stopped, /session reset/i);
    assert.equal(await request('eval',"exists('x')"), '[1] FALSE');
    assert.deepEqual(errors, []);
    console.log('Browser Rust worker: UI evaluation, typed strings, recoverable errors, nonlocal control flow, PNG plotting, cancellation and reset passed');
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; }).finally(() => server.close());
