#!/usr/bin/env node
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node quote-options.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixtures = path.join(__dirname, '../../crates/r-embed/tests/fixtures');
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval_checked(`identical(Sys.getlocale("LC_CTYPE"), "C.UTF-8") &&
      isTRUE(l10n_info()[["UTF-8"]]) && identical(l10n_info()$codeset, "UTF-8") &&
      identical(Sys.setlocale("LC_CTYPE", "C"), "") && isTRUE(l10n_info()[["UTF-8"]])`), '[1] TRUE\n');
    for (const name of ['quote-options-contract', 'unicode-print-width-contract']) {
      assert.equal(session.eval_checked(fs.readFileSync(path.join(fixtures, name + '.R'), 'utf8')),
        fs.readFileSync(path.join(fixtures, name + '.out'), 'utf8'));
    }
    assert.equal(session.eval_checked('1L + 1L'), '[1] 2\n');
  } finally { session.close(); session.free(); }
}
console.log('QUOTE_OPTIONS_WASM_COMPLETE checks=8 failures=0');
