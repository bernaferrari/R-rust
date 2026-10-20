#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node s4-cache-new.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixtures = path.join(__dirname, '../../crates/r-embed/tests/fixtures');
const source = fs.readFileSync(path.join(fixtures, 's4-cache-new-contract.R'), 'utf8');
const expected = fs.readFileSync(path.join(fixtures, 's4-cache-new-contract.out'), 'utf8');
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval(source), expected);
    assert.equal(session.eval('identical(1L + 1L, 2L)'), '[1] TRUE\n');
  } finally { session.close(); }
}
console.log('S4_CACHE_NEW_WASM_COMPLETE checks=4 failures=0');
