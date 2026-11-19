#!/usr/bin/env node
const assert = require('node:assert/strict');
const path = require('node:path');
function checkImportBoundaries(session) {
  const maxFileBytes = 1024 * 1024;
  const buffer = new Uint8Array(maxFileBytes + 2);
  buffer[1] = 42;
  session.import_file('binary/full.bin', buffer.subarray(1, maxFileBytes + 1));
  assert.throws(() => session.import_file('binary/full.bin', buffer), /byte limit/);
  assert.throws(() => session.import_file('x'.repeat(256), buffer.subarray(0, 1)), /relative paths/);
  const full = session.export_file('binary/full.bin');
  assert.equal(full.length, maxFileBytes);
  assert.equal(full[0], 42);
  assert.equal(full[maxFileBytes - 1], 0);
  session.remove_file('binary/full.bin');
  return 5;
}

function checkConnectionSnapshots(session) {
  session.import_file('binary/snapshots.bin', Uint8Array.of(97, 10));
  session.eval_checked(`
    reader <- file('binary/snapshots.bin', 'rb')
    writer <- file('binary/snapshots.bin', 'ab')
    writeBin(as.raw(c(98, 10)), writer)
    close(writer)
    stopifnot(identical(readBin(reader, 'raw', 4L), as.raw(c(97, 10))))
    close(reader)
    reader <- file('binary/snapshots.bin', 'r')
    stopifnot(identical(readLines(reader), c('a', 'b')))
    close(reader)
    rm(reader, writer)
  `);
  assert.deepEqual(session.export_file('binary/snapshots.bin'), Uint8Array.of(97, 10, 98, 10));
  session.remove_file('binary/snapshots.bin');
  return 3;
}

module.exports = { checkImportBoundaries };

if (require.main === module) {
  if (process.argv.length !== 3) throw new Error('Usage: node binary-admission.cjs WASM_PACKAGE');
  const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
  let checks = 0;
  for (let generation = 0; generation < 2; generation++) {
    const session = new WasmRSession();
    try {
      session.import_file('binary/input.bin', Uint8Array.of(9,8,7,6));
      checks += checkImportBoundaries(session);
      checks += checkConnectionSnapshots(session);
      for (const source of [
        "identical(readBin(con=as.raw(c(9,8,7,6)), 'raw', 2L), as.raw(c(9,8)))",
        "identical(writeBin(con=raw(), c(1L,256L), size=2L, endian='big'), as.raw(c(0,1,1,0)))",
        "identical(readBin(what='raw', con='binary/input.bin', 2L), as.raw(c(9,8)))",
      ]) {
        assert.equal(session.eval(source), '[1] TRUE\n', source);
        checks++;
      }
    } finally {
      session.close();
      session.free();
    }
  }
  console.log(`BINARY_ADMISSION_WASM_COMPLETE checks=${checks} failures=0`);
}
