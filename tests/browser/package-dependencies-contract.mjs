import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const pkg = process.argv[2];
assert.ok(pkg, 'Expected the production Wasm package directory');
const module = readFileSync(resolve(pkg, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(pathToFileURL(resolve(pkg, 'r_wasm.js')));
initSync({ module });

const caseFile = new URL('../conformance/cases/1219_tools_package_dependencies.R', import.meta.url);
const outputFile = new URL('../conformance/golden/1219_tools_package_dependencies.out', import.meta.url);
const s4 = new URL('../../crates/r-embed/tests/fixtures/s4-next-method-public-contract', import.meta.url);
const program = readFileSync(caseFile, 'utf8');
const expected = readFileSync(outputFile, 'utf8');
const session = new WasmRSession();

try {
  // Exercise repeated real dependency resolution before using S4 in the
  // same session, under the standard interpreter resource limits.
  for (let attempt = 0; attempt < 4; attempt++) {
    assert.equal(session.eval_checked(program), expected);
  }
  assert.equal(session.eval_checked(readFileSync(new URL(s4.href + '.R'), 'utf8')),
    readFileSync(new URL(s4.href + '.out'), 'utf8'));
  console.log(JSON.stringify({
    wasm_sha256: createHash('sha256').update(module).digest('hex'),
    dependency_scans_passed: 4,
    s4_after_dependency_scans: 'pass',
  }));
} finally {
  session.free();
}
