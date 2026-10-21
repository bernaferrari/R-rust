#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node regex-replacement-missing.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixtures = ['regex-replacement-missing-contract.R', 'regex-replacement-case-contract.R', 'regex-unicode-public-contract.R']
  .map(name => fs.readFileSync(path.join(__dirname, '../../crates/r-embed/tests/fixtures', name), 'utf8'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    for (const fixture of fixtures) {
      assert.equal(session.eval_checked(fixture), '[1] TRUE\n'); checks++;
    }
    assert.equal(session.eval_checked('1L+1L'), '[1] 2\n'); checks++;
  } finally { session.close(); session.free(); }
}
assert.equal(checks, 8);
console.log(`REGEX_REPLACEMENT_MISSING_WASM_COMPLETE checks=${checks} failures=0`);
