import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [packageDirectory, fixture, outputDirectory] = process.argv.slice(2);
assert.ok(packageDirectory && fixture && outputDirectory,
  'Expected the production package, fixture and output directories');
const moduleBytes = readFileSync(resolve(packageDirectory, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(
  pathToFileURL(resolve(packageDirectory, 'r_wasm.js'))
);
initSync({ module: moduleBytes });

for (const [script, files] of Object.entries({
  plot: ['panels.pdf', 'panels-live.pdf', 'panels-replay.pdf', 'panels.rds'],
  restore: ['panels-restored.pdf'],
  absolute: ['source.pdf', 'replay.pdf'],
  recording: ['source.pdf', 'without.pdf', 'with.pdf', 'inhibited.pdf'],
  safety: ['safety.pdf'],
})) {
  const folder = script === 'restore' ? 'plot' : script;
  mkdirSync(resolve(outputDirectory, folder), { recursive: true });
  const session = new WasmRSession();
  try {
    if (script === 'restore') {
      session.import_file('panels.rds', readFileSync(resolve(outputDirectory, folder, 'panels.rds')));
    }
    session.eval_checked(readFileSync(resolve(fixture, `${script}.R`), 'utf8'));
    for (const name of files) {
      writeFileSync(resolve(outputDirectory, folder, name), session.export_file(name));
    }
  } finally {
    session.free();
  }
}
console.log(createHash('sha256').update(moduleBytes).digest('hex'));
