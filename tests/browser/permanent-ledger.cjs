#!/usr/bin/env node
// Exercise permanent symbol pressure through the production 64 MiB facade.
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node permanent-ledger.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval('identical(as.name("retained"), as.name("retained"))'), '[1] TRUE\n');
    checks++;
    // Build the R input before interning: the new name's simultaneous native
    // copies must compete with that retained input under the existing cap.
    assert.equal(session.eval('symbol_name <- strrep("x", 20000000L); invisible(NULL)'), '');
    checks++;
    const result = session.eval('invisible(as.name(symbol_name))');
    assert.match(result, /permanent symbol allocation exceeds session memory limit/);
    checks++;
    assert.equal(session.eval('gc(); identical(as.name("retained"), as.name("retained"))').trim().endsWith('[1] TRUE'), true);
    checks++;
    assert.equal(session.eval('identical(1L + 1L, 2L)'), '[1] TRUE\n');
    checks++;
  } finally {
    session.close();
  }
}
console.log(`PERMANENT_LEDGER_WASM_COMPLETE checks=${checks} failures=0`);
