import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [packageDirectory, library] = process.argv.slice(2);
assert.ok(packageDirectory && library, 'Expected the production Wasm package and source library');
const moduleBytes = readFileSync(resolve(packageDirectory, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(
  pathToFileURL(resolve(packageDirectory, 'r_wasm.js')).href
);
initSync({ module: moduleBytes });
const contract = new URL('../everyday-analysis/sessions/pure-r-package/dependencies.R', import.meta.url);
const session = new WasmRSession();

try {
  for (const root of ['pinned', 'synthetic', 'wrappers']) {
    for (const pkg of readdirSync(resolve(library, root))) {
      const location = `${root}/${pkg}`;
      for (const name of ['DESCRIPTION', 'NAMESPACE']) {
        session.import_file(`${location}/${name}`, readFileSync(resolve(library, location, name)));
      }
      for (const name of readdirSync(resolve(library, location, 'R')).sort()) {
        if (name.endsWith('.R')) {
          session.import_file(`${location}/R/${name}`, readFileSync(resolve(library, location, 'R', name)));
        }
      }
    }
  }
  console.log(JSON.stringify({
    wasm_sha256: createHash('sha256').update(moduleBytes).digest('hex'),
    dependency_output: session.eval_checked(readFileSync(contract, 'utf8')),
  }));
} finally {
  session.free();
}
