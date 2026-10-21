#!/usr/bin/env node
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node printer-workspace.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval_checked('resident <- raw(16777216L); invisible(NULL)'), ''); checks++;
    assert.equal(session.eval_checked('identical(encodeString("ok",width=4L,justify="right"),"  ok")'), '[1] TRUE\n'); checks++;
    // Legal field width. The original resident allocation stays live, and the
    // full native replacement cannot fit the unchanged64MiB session budget.
    assert.equal(session.eval_string('tryCatch({encodeString("x",width=50331648L);"unexpected success"},error=function(e)conditionMessage(e))'), 'failed to allocate encoded string buffer'); checks++;
    assert.equal(session.eval_checked('identical(length(resident),16777216L)'), '[1] TRUE\n'); checks++;
    assert.equal(session.eval_checked('identical(encodeString("ok",width=4L,justify="right"),"  ok")'), '[1] TRUE\n'); checks++;
    assert.equal(session.eval_checked('1L+1L'), '[1] 2\n'); checks++;
  } finally { session.close(); session.free(); }
}
console.log(`PRINTER_WORKSPACE_WASM_COMPLETE checks=${checks} failures=0`);
