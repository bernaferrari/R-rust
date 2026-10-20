#!/usr/bin/env node
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node binary-admission.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    session.import_file('binary/input.bin', Uint8Array.of(9,8,7,6));
    for (const source of [
      "identical(readBin(con=as.raw(c(9,8,7,6)), 'raw', 2L), as.raw(c(9,8)))",
      "identical(writeBin(con=raw(), c(1L,256L), size=2L, endian='big'), as.raw(c(0,1,1,0)))",
      "identical(readBin(what='raw', con='binary/input.bin', 2L), as.raw(c(9,8)))",
    ]) {
      assert.equal(session.eval(source), '[1] TRUE\n', source);
      checks++;
    }
  } finally { session.close(); }
}
console.log(`BINARY_ADMISSION_WASM_COMPLETE checks=${checks} failures=0`);
