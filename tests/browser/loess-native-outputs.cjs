#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node loess-native-outputs.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixtures = path.join(__dirname, '../../crates/r-embed/tests/fixtures');
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    session.import_file('loess-native-oracle.rds', fs.readFileSync(path.join(fixtures, 'gnu-loess-native-outputs.rds')));
    session.eval_checked("expected <- readRDS('loess-native-oracle.rds')");
    session.eval_checked(fs.readFileSync(path.join(fixtures, 'loess-native-outputs-input.R'), 'utf8'));
    assert.equal(session.eval_checked("actual <- loess_native_outputs(); comparison <- all.equal(actual, expected, tolerance=1e-9); if (!isTRUE(comparison)) stop(paste(comparison, collapse='; ')); identical(length(actual), 40L)"), '[1] TRUE\n');
    checks += 40;
    assert.equal(session.eval_checked(fs.readFileSync(path.join(fixtures, 'loess-isolation.R'), 'utf8')), '[1] TRUE\n');
    checks++;
    assert.equal(session.eval_checked('1L + 1L'), '[1] 2\n');
    checks++;
  } finally { session.close(); session.free(); }
}
assert.equal(checks, 84);
console.log(`LOESS_NATIVE_OUTPUTS_WASM_COMPLETE checks=${checks} failures=0`);
