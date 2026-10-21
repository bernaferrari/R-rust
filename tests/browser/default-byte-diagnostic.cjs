#!/usr/bin/env node
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node default-byte-diagnostic.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
(async () => {
  const { runDefaultByteDiagnostics } = await import('./default-byte-diagnostic-contract.mjs');
  let checks = 0;
  for (let generation = 0; generation < 2; generation++) {
    const session = new WasmRSession();
    try { checks += runDefaultByteDiagnostics(session, generation); }
    finally { session.close(); session.free(); }
  }
  assert.equal(checks, 78);
  console.log(`DEFAULT_BYTE_DIAGNOSTIC_WASM_COMPLETE checks=${checks} failures=0`);
})().catch(error => { console.error(error); process.exitCode = 1; });
