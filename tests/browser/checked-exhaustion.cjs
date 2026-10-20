#!/usr/bin/env node
// Native paste scratch fits, but its final R string competes for the same cap.
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node checked-exhaustion.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval('prefix <- strrep("x",2097152L); invisible(NULL)'), '');
    checks++;
    // The Wasm gc table counts vector cells in four-byte projection units.
    const used = Number(session.eval_string('as.character(gc()[2,1])')) * 4;
    const filler = 64 * 1024 * 1024 - used - 10 * 1024 * 1024;
    assert.ok(Number.isSafeInteger(filler) && filler > 0);
    checks++;
    assert.equal(session.eval(`filler <- raw(${filler}L); invisible(NULL)`), '');
    checks++;
    assert.equal(session.eval_string(`tryCatch({
      paste0(prefix,prefix)
      "unexpected success"
    }, error=function(e) conditionMessage(e))`), 'failed to allocate R value');
    checks++;
    assert.equal(session.eval('identical(1L+1L,2L)'), '[1] TRUE\n');
    checks++;
    assert.equal(session.eval('identical(paste0("safe", " recovery"), "safe recovery")'), '[1] TRUE\n');
    checks++;
  } finally { session.close(); }
}
console.log(`CHECKED_EXHAUSTION_WASM_COMPLETE checks=${checks} failures=0`);
