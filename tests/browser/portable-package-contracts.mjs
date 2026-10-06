import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));
assert.ok(process.argv[2], 'Usage: node portable-package-contracts.mjs <production-wasm-package>');
const packageDirectory = resolve(process.argv[2]);
const moduleBytes = readFileSync(resolve(packageDirectory, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(
  new URL('r_wasm.js', pathToFileURL(packageDirectory + sep)).href
);
initSync({ module: moduleBytes });
console.log(JSON.stringify({ wasm_sha256: createHash('sha256').update(moduleBytes).digest('hex') }));

const fixtures = resolve(root, 'crates/r-embed/tests/fixtures');
const contracts = [
  ...[
    '260_length_replacement',
    '492_rep_int_len_helpers',
    '288_with_calling_handlers',
    '1094_cat_complex_fft',
    '544_complex_constructor_format_str_parity',
    '551_round_signif_complex_parity',
  ].map(name => [
    resolve(root, 'tests/conformance/cases', name + '.R'),
    ['260_length_replacement', '492_rep_int_len_helpers', '288_with_calling_handlers'].includes(name)
      ? resolve(root, 'tests/conformance/golden', name + '.out')
      : resolve(fixtures, 'complex-original-' + name + '.out'),
  ]),
  ...[
    'complex-print-public-contract',
    'stats-namespace-public-contract',
    'get-lazy-mode-public-contract',
    'correlation-public-contract',
    'palette-public-contract',
    'tempfile-public-contract',
    'bincode-public-contract',
    'print-gap-public-contract',
    'arima0-public-contract',
    'utils-console-public-contract',
    'mapply-public-contract',
    'sample-condition-contract',
    'rep-len-admission-contract',
    'calling-error-handler-public-contract',
    'calling-warning-handler-public-contract',
    'unserialize-connection-contract',
  ].map(name => [resolve(fixtures, name + '.R'), resolve(fixtures, name + '.out')]),
];

const temporaryNames = new Set();
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    for (const [source, expected] of contracts) {
      try {
        assert.equal(session.eval_checked(readFileSync(source, 'utf8')),
          readFileSync(expected, 'utf8'), `generation=${generation}, source=${source}`);
      } catch (error) {
        throw new Error(`generation=${generation}, source=${source}`, { cause: error });
      }
      assert.equal(session.eval_checked('1 + 1'), '[1] 2\n');
    }
    const name = session.eval_checked("tempfile(pattern='rport',fileext='.tmp')");
    assert.equal(temporaryNames.has(name), false, 'Uncreated names must remain distinct across fresh sessions');
    temporaryNames.add(name);
    console.log(JSON.stringify({ generation, contracts_passed: contracts.length }));
  } finally {
    session.close();
    session.free();
  }
}
