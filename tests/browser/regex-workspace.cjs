#!/usr/bin/env node
// Native capture growth competes with the production facade's original 64 MiB cap.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node regex-workspace.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const contract = fs.readFileSync(path.join(__dirname,
  '../../crates/r-embed/tests/fixtures/regex-unicode-public-contract.R'), 'utf8');
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    assert.equal(session.eval_checked(contract), '[1] TRUE\n');
    checks++;
    assert.equal(session.eval_string(`local({
      subject <- strrep('a', 800000L)
      tryCatch({
        gregexpr('(a)()', subject, perl=TRUE)
        'unexpected success'
      }, error=function(e) conditionMessage(e))
    })`), 'regex workspace exceeds session memory limit');
    checks++;
    assert.equal(session.eval_checked("identical(regexec('(a)', 'a', perl=TRUE)[[1L]], structure(c(1L,1L), match.length=c(1L,1L), useBytes=TRUE, index.type='chars'))"), '[1] TRUE\n');
    checks++;
    assert.equal(session.eval_checked('1L + 1L'), '[1] 2\n');
    checks++;
  } finally { session.close(); session.free(); }
}
assert.equal(checks, 8);
console.log(`REGEX_WORKSPACE_WASM_COMPLETE checks=${checks} failures=0`);
