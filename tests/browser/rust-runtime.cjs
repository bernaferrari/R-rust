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
    assert.equal(await request('eval', "xx<-seq(0,1,length.out=15); yy<-sin(5*xx)+xx^2; ff<-loess(yy~xx); round(ff$fitted[1],8)"), '[1] -0.02619684');
    const smooth=await request('plot', "plot(xx,yy,main='LOESS μ'); lines(xx,predict(ff),col='red',lwd=3)");
    assert.match(smooth,/data:image\/png;base64,/);
    if (process.env.RPORT_PLOT_ARTIFACT) await fs.writeFile(process.env.RPORT_PLOT_ARTIFACT,Buffer.from(smooth.match(/data:image\/png;base64,([^"]+)/)[1],'base64'));
    const ink=await page.evaluate(async src=>{
      const img=new Image();img.src=new DOMParser().parseFromString(src,'image/svg+xml').querySelector('image').getAttribute('href');await img.decode();
      const canvas=document.createElement('canvas');canvas.width=img.width;canvas.height=img.height;
      const ctx=canvas.getContext('2d');ctx.drawImage(img,0,0);
      const data=ctx.getImageData(0,0,img.width,42).data;let n=0;
      for(let i=0;i<data.length;i+=4) if(data[i]<200&&data[i+3]>0)n++;
      return n;
    },smooth);
    assert.ok(ink>30,'bundled font renders a title in Wasm');
    for (const code of [
      "hist(c(0.1,0.2,0.8,1.2,1.9),col='skyblue')",
      "hist(c(0.1,0.2,0.8,1.2,1.9),breaks=c(0,1,2),col='skyblue')",
      "barplot(c(2,4,3),col='gold')",
      "boxplot(c(1,2,3,4,100),col='skyblue')",
      "plot(0:1,0:1,type='n'); rasterImage(matrix(c('red','blue','green','white'),2),0,0,1,1,interpolate=FALSE)",
      "plot(1:3,3:1,pch=21,bg='gold'); savedPlot<-serialize(recordPlot(),NULL); replayPlot(unserialize(savedPlot))",
    ]) assert.match(await request('plot',code), /data:image\/png;base64,/);
    assert.equal(await request('eval', "nx<-seq(0,1,length.out=30); ny<-sin(nx); ny[c(4,17)]<-NA; nf<-loess(ny~nx,na.action=na.exclude); np<-predict(nf); paste(length(np),paste(which(is.na(np)),collapse=','),any(is.nan(np)),sep='|')"), '[1] "30|4,17|FALSE"');
    await assert.rejects(request('eval', "lx<-seq(0,1,length.out=5000); ly<-sin(lx); loess(ly~lx)"), /LOESS workspace limit exceeded/);
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
    console.log('Browser Rust worker: UI evaluation, typed strings, recoverable errors, nonlocal control flow, Vello PNG plotting, hist/bar/box/raster, serialized replay, cancellation and reset passed');
  } finally { await browser.close(); }
})().catch(e => { console.error(e); process.exitCode = 1; }).finally(() => server.close());
