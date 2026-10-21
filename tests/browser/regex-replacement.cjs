#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node regex-replacement.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const contract = fs.readFileSync(path.join(__dirname,
  '../../crates/r-embed/tests/fixtures/regex-replacement-case-contract.R'), 'utf8');
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval_checked(contract), '[1] TRUE\n'); checks++;
    assert.equal(session.eval_checked('resident <- raw(25165824L); subject <- strrep("a",8388608L); invisible(NULL)'), ''); checks++;
    // A legal32MiB result competes with the retained24MiB raw and8MiB subject.
    // Reject the native old+new output peak before allocating a replacement.
    assert.equal(session.eval_string(String.raw`tryCatch({gsub("(a+)","\\1\\1\\1\\1",subject,perl=TRUE);"unexpected success"},error=function(e)conditionMessage(e))`),
      'regex workspace exceeds session memory limit'); checks++;
    assert.equal(session.eval_checked('identical(c(length(resident),nchar(subject)),c(25165824L,8388608L))'), '[1] TRUE\n'); checks++;
    assert.equal(session.eval_checked(String.raw`identical(sub("(a)","\\U\\1","a",perl=TRUE),"A")`), '[1] TRUE\n'); checks++;
    assert.equal(session.eval_checked('1L+1L'), '[1] 2\n'); checks++;
  } finally { session.close(); session.free(); }
}
assert.equal(checks, 12);
console.log(`REGEX_REPLACEMENT_WASM_COMPLETE checks=${checks} failures=0`);
