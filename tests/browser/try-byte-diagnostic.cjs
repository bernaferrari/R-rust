#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node try-byte-diagnostic.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixture = fs.readFileSync(path.join(__dirname, '../../crates/r-embed/tests/fixtures/try-byte-diagnostic-contract.R'), 'utf8');
const construction = fs.readFileSync(path.join(__dirname, '../../crates/r-embed/tests/fixtures/try-construction-contract.R'), 'utf8');
const cases = [fixture, construction].flatMap(source => source.split('\n').filter(line => line.startsWith('stopifnot(')));
assert.equal(cases.length, 23);
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    for (const code of cases) {
      assert.equal(session.eval_checked(code + '\nTRUE'), '[1] TRUE\n'); checks++;
    }
    for (const source of [fixture, construction]) { assert.equal(session.eval_checked(source), '[1] TRUE\n'); checks++; }
    assert.equal(session.eval_checked('1L+1L'), '[1] 2\n'); checks++;
  } finally { session.close(); session.free(); }
}
assert.equal(checks, 52);
console.log(`TRY_BYTE_DIAGNOSTIC_WASM_COMPLETE checks=${checks} failures=0`);
