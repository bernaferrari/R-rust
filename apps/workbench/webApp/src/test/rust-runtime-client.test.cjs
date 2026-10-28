const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { test } = require('node:test');
const { runInNewContext } = require('node:vm');

function client() {
  const workers = [];
  class Worker {
    constructor() { workers.push(this); }
    postMessage(request) { this.request = request; }
    terminate() { this.terminated = true; }
    reply(data) { this.onmessage({ data: { id: this.request.id, ...data } }); }
  }
  const context = { Worker, URL, document: { baseURI: 'https://example.test/' } };
  runInNewContext(readFileSync(join(__dirname, '../wasmJsMain/resources/rust-runtime-client.js'), 'utf8'), context);
  return { runtime: context.rportRust, workers };
}

test('stale worker error cannot affect a replacement session', async () => {
  const { runtime, workers } = client();
  const stopped = runtime.request('eval', '1');
  const rejected = assert.rejects(stopped, /Stopped/);
  runtime.cancel();
  await rejected;
  const next = runtime.request('eval', '2');
  const replacement = workers[1];
  workers[0].onerror();
  assert.equal(replacement.terminated, undefined);
  replacement.reply({ value: '2' });
  assert.equal(await next, '2');
});

test('active worker failure rejects all requests and permits restart', async () => {
  const { runtime, workers } = client();
  const first = assert.rejects(runtime.request('eval', '1'), /worker failed/);
  const second = assert.rejects(runtime.request('eval', '2'), /worker failed/);
  workers[0].onerror();
  await Promise.all([first, second]);
  assert.equal(workers[0].terminated, true);
  const restarted = runtime.request('eval', '3');
  workers[1].reply({ value: '3' });
  assert.equal(await restarted, '3');
});

test('active fatal response rejects queued requests and permits restart', async () => {
  const { runtime, workers } = client();
  const first = assert.rejects(runtime.request('eval', '1'), /panic/);
  const firstId = workers[0].request.id;
  const second = assert.rejects(runtime.request('eval', '2'), /panic/);
  workers[0].onmessage({ data: { id: firstId, fatal: true, error: 'panic' } });
  await Promise.all([first, second]);
  assert.equal(workers[0].terminated, true);
  const restarted = runtime.request('eval', '3');
  workers[1].reply({ value: '3' });
  assert.equal(await restarted, '3');
});
