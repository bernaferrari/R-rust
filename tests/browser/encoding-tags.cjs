#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node encoding-tags.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixtures = path.join(__dirname, '../../crates/r-embed/tests/fixtures');
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval_checked(fs.readFileSync(path.join(fixtures, 'encoding-tag-contract.R'), 'utf8')),
      fs.readFileSync(path.join(fixtures, 'encoding-tag-contract.out'), 'utf8'));
    assert.equal(session.eval_checked('1L + 1L'), '[1] 2\n');
  } finally { session.close(); session.free(); }
}
console.log('ENCODING_TAGS_WASM_COMPLETE checks=4 failures=0');
