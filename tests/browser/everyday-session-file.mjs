import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [packageDirectory, fixture] = process.argv.slice(2);
assert.ok(packageDirectory && fixture, 'Expected the production package and fixture directories');
const moduleBytes = readFileSync(resolve(packageDirectory, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(
  pathToFileURL(resolve(packageDirectory, 'r_wasm.js')).href
);
initSync({ module: moduleBytes });
const digest = bytes => createHash('sha256').update(bytes).digest('hex');

const first = new WasmRSession();
let imported;
let exported;
try {
  first.import_file('input.csv', readFileSync(resolve(fixture, 'input.csv')));
  imported = first.eval_checked(readFileSync(resolve(fixture, 'import.R'), 'utf8'));
  exported = first.export_file('export.csv');
} finally {
  first.free();
}

const second = new WasmRSession();
let reloaded;
try {
  assert.deepEqual(second.list_files(), []);
  second.import_file('export.csv', exported);
  reloaded = second.eval_checked(readFileSync(resolve(fixture, 'reload.R'), 'utf8'));
} finally {
  second.free();
}

console.log(JSON.stringify({
  wasm_sha256: digest(moduleBytes),
  outputs: [imported, reloaded],
  export_sha256: digest(exported),
}));
