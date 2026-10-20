#!/usr/bin/env node
// Exercise permanent symbol pressure through the production 64 MiB facade.
const assert = require('node:assert/strict');
const path = require('node:path');
const oracle = require('node:fs').readFileSync(path.join(__dirname, '../symbols/length-oracle.R'), 'utf8');
if (process.argv.length !== 3) throw new Error('Usage: node permanent-ledger.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval(oracle), '[1] TRUE\n');
    checks++;
    assert.equal(session.eval('identical(as.name("retained"), as.name("retained"))'), '[1] TRUE\n');
    checks++;
    const before = Number(session.eval_string('as.character(gc()[2,1])')) * 4;
    assert.equal(session.eval(`prefix <- strrep("x",9990L)
      for(i in 1:2000) {
        invisible(as.name(paste0(prefix,i)))
        if(i%%100L==0L) gc()
      }
      invisible(NULL)`), '');
    checks++;
    const after = Number(session.eval_string('as.character(gc()[2,1])')) * 4;
    assert.ok(after > before + 16*1024*1024, 'permanent names escaped the public byte ledger');
    checks++;
    const denied = session.eval_string(`tryCatch({
      for(i in 2001:10000) {
        invisible(as.name(paste0(prefix,i)))
        if(i%%100L==0L) gc()
      }
      "symbols escaped the limit"
    }, error=function(e) conditionMessage(e))`);
    assert.match(denied, /memory|allocation|allocate|limit/);
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
