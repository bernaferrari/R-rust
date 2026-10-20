#!/usr/bin/env node
// Production browser facade, including its unchanged 64 MiB session limit.
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node serialization-workspace.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval('identical(file.exists("state.rds"), FALSE)'), '[1] TRUE\n');
    checks++;
    for (const codec of ['FALSE', 'TRUE', '"gzip"', '"bzip2"', '"xz"']) {
      assert.equal(session.eval(`local({
        value <- list(numbers=1:64, label="Rust é", empty=character())
        saveRDS(value, "state.rds", compress=${codec})
        identical(readRDS("state.rds"), value)
      })`), '[1] TRUE\n', `${codec}, generation ${generation}`);
      checks++;
    }
    assert.equal(session.eval(`local({
      denied <- tryCatch({ numeric(100000000); FALSE }, error=function(e) TRUE)
      stopifnot(denied)
      identical(1L + 1L, 2L)
    })`), '[1] TRUE\n');
    checks++;
  } finally {
    session.close();
  }
}
console.log(`SERIALIZATION_WORKSPACE_WASM_COMPLETE checks=${checks} failures=0`);
