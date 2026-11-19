import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [packageDirectory, fixture, scratch] = process.argv.slice(2);
assert.ok(packageDirectory && fixture && scratch, 'Expected package, test fixture and scratch paths');
const moduleBytes = readFileSync(resolve(packageDirectory, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(
  pathToFileURL(resolve(packageDirectory, 'r_wasm.js')).href
);
initSync({ module: moduleBytes });

const file = 'analysis.RData';
const saveScript = readFileSync(resolve(fixture, 'save.R'), 'utf8');
const reloadScript = readFileSync(resolve(fixture, 'reload.R'), 'utf8');
const writer = new WasmRSession();
let saved;
let ownWorkspace;
try {
  saved = writer.eval_checked(saveScript);
  ownWorkspace = writer.export_file(file);
  writeFileSync(resolve(scratch, 'wasm.RData'), ownWorkspace);
} finally {
  writer.free();
}

const restored = {};
for (const origin of ['gnu', 'native', 'wasm']) {
  const bytes = origin === 'wasm' ? ownWorkspace
    : readFileSync(resolve(scratch, origin + '.RData'));
  const reader = new WasmRSession();
  try {
    assert.deepEqual(reader.list_files(), []);
    reader.import_file(file, bytes);
    restored[origin] = reader.eval_checked(reloadScript);
  } finally {
    reader.free();
  }
}
console.log(JSON.stringify({
  wasm_sha256: createHash('sha256').update(moduleBytes).digest('hex'),
  saved,
  restored,
}));
