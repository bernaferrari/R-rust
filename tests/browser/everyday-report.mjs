import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [pkg, fixture, destination] = process.argv.slice(2);
const module = readFileSync(resolve(pkg, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(pathToFileURL(resolve(pkg, 'r_wasm.js')));
initSync({ module });
const driver = readFileSync(resolve(fixture, 'run.R'), 'utf8');
const outputs = ['index.html', 'forecast.csv', 'field-trial.pdf'];
const stdout = {};

for (const phase of ['initial', 'rebuild']) {
  const session = new WasmRSession();
  const folder = resolve(destination, phase);
  mkdirSync(folder);
  try {
    session.import_file('report.R', readFileSync(resolve(fixture, 'report.R')));
    session.import_file('observations.csv', readFileSync(resolve(fixture, phase + '.csv')));
    if (phase === 'rebuild') {
      for (const name of outputs) {
        session.import_file(name, readFileSync(resolve(destination, 'initial', name)));
      }
    }
    stdout[phase] = session.eval_checked(driver);
    for (const name of outputs) {
      writeFileSync(resolve(folder, name), session.export_file(name));
    }
  } finally {
    session.free();
  }
}
console.log(JSON.stringify({
  wasm_sha256: createHash('sha256').update(module).digest('hex'),
  stdout,
}));
