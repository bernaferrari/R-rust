#!/usr/bin/env node
// Run the same pinned GNU contracts through the production Wasm facade.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

if (process.argv.length !== 3) throw new Error('Usage: node connection-streams.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const root = path.resolve(__dirname, '../..');
const source = relative => fs.readFileSync(path.join(root, relative), 'utf8');
let session;
let checks = 0;
try {
  const compressed = fs.readFileSync(path.join(root, 'crates/r-embed/tests/fixtures/gnu-sequential-v2.rds.gz'));
  const compiled = fs.readFileSync(path.join(root, 'crates/r-embed/tests/fixtures/gnu-bytecode-stream-hooks-v2.rds'));
  for (let generation = 0; generation < 2; generation++) {
    if (session) session.close();
    session = new WasmRSession();
    const contracts = [
      ['constructor', source('crates/rmath/src/mainutils/connections/file/constructor-contract.R')],
      ...['sequential', 'hook-cursor', 'hook-reentry'].map(name => [name, source(`crates/rmath/src/mainutils/serialize/${name}-contract.R`)]),
      ['compressed', `compressed <- as.raw(c(${[...compressed].join(',')}));${source('crates/rmath/src/mainutils/serialize/compressed-stream-contract.R')}`],
      ['compiled', `compiled <- as.raw(c(${[...compiled].join(',')}));${source('crates/rmath/src/mainutils/serialize/compiled-hook-contract.R')}`],
    ];
    for (const [name, program] of contracts) {
      assert.equal(session.eval(`{${program}\n}`), '[1] TRUE\n', `${name} generation ${generation}`);
      checks++;
    }
  }
  console.log(`CONNECTION_STREAM_WASM_COMPLETE checks=${checks} failures=0`);
} finally {
  if (session) session.close();
}
