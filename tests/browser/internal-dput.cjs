#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node internal-dput.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixtures = path.join(__dirname, '../../crates/r-embed/tests/fixtures');
const source = fs.readFileSync(path.join(fixtures, 'internal-dput-contract.R'), 'utf8');
const expected = fs.readFileSync(path.join(fixtures, 'internal-dput-contract.out'), 'utf8');
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval_checked(source), expected);
    assert.equal(session.eval_checked('1L + 1L'), '[1] 2\n');
  } finally { session.close(); session.free(); }
}
console.log('INTERNAL_DPUT_WASM_COMPLETE checks=4 failures=0');
