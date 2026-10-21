#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node regex-compile-error-recovery.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const contracts = ['regex-compile-error-recovery-contract.R', 'regex-empty-fixed-contract.R']
  .map(name => fs.readFileSync(path.join(__dirname, '../../crates/r-embed/tests/fixtures', name), 'utf8'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    for (const contract of contracts) {
      assert.equal(session.eval_checked(contract), '[1] TRUE\n'); checks++;
    }
    assert.equal(session.eval_checked(String.raw`identical(gsub("([a-z]+)","\\U\\1",c("cat","dog"),perl=TRUE),c("CAT","DOG"))`), '[1] TRUE\n'); checks++;
    assert.equal(session.eval_checked('identical(regmatches("cat",regexec("([a-z]+)","cat")),list(c("cat","cat")))'), '[1] TRUE\n'); checks++;
    assert.equal(session.eval_checked('1L+1L'), '[1] 2\n'); checks++;
  } finally { session.close(); session.free(); }
}
assert.equal(checks, 10);
console.log(`REGEX_COMPILE_ERROR_RECOVERY_WASM_COMPLETE checks=${checks} failures=0`);
