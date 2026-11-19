import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [packageDirectory, fixture, outputDirectory] = process.argv.slice(2);
assert.ok(packageDirectory && fixture && outputDirectory,
  'Expected the production package, fixture and output directories');
const moduleBytes = readFileSync(resolve(packageDirectory, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(
  pathToFileURL(resolve(packageDirectory, 'r_wasm.js')).href
);
initSync({ module: moduleBytes });
const session = new WasmRSession();

try {
  const output = session.eval_checked(readFileSync(resolve(fixture, 'plot.R'), 'utf8'));
  const exported = session.export_file('weekly-observations.pdf');
  writeFileSync(resolve(outputDirectory, 'weekly-observations.pdf'), exported);
  session.import_file('protected.pdf', exported);
  const safety = session.eval_checked(readFileSync(resolve(fixture, 'safety.R'), 'utf8'));
  const preserved = session.export_file('protected.pdf');
  assert.deepEqual(preserved, exported);
  console.log(JSON.stringify({
    wasm_sha256: createHash('sha256').update(moduleBytes).digest('hex'),
    output,
    safety,
    preserved_sha256: createHash('sha256').update(preserved).digest('hex'),
  }));
} finally {
  session.free();
}
