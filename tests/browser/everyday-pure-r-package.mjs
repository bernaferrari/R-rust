import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const packageDirectory = process.argv[2];
assert.ok(packageDirectory, 'Expected the production Wasm package directory');
const moduleBytes = readFileSync(resolve(packageDirectory, 'r_wasm_bg.wasm'));
const { WasmRSession, initSync } = await import(
  pathToFileURL(resolve(packageDirectory, 'r_wasm.js')).href
);
initSync({ module: moduleBytes });

const fixtureRoot = new URL('../real-packages/', import.meta.url);
const archive = fileURLToPath(new URL('vendor/R6_2.6.1.tar.gz', fixtureRoot));
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
assert.equal(sha256(readFileSync(archive)),
  '59c6eba8b1b912eb7e104f65053235604be853425ee67c152ac4e86a1f2073b4');
const readArchiveFile = name => execFileSync('tar', ['-xOzf', archive, name]);
const sources = execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' })
  .split('\n').filter(name => /^R6\/R\/[^/]+\.R$/.test(name)).sort();
assert.ok(sources.length > 0, 'The pinned R6 archive must contain R sources');

const session = new WasmRSession();
let result;
try {
  session.eval_checked('.libPaths("packages")');
  assert.throws(() => session.eval_checked('library(R6)'), /no package called 'R6'/);
  session.import_file('packages/R6/DESCRIPTION', readArchiveFile('R6/DESCRIPTION'));
  assert.throws(() => session.eval_checked('library(R6)'), /has no NAMESPACE/);
  session.import_file('packages/R6/NAMESPACE', readArchiveFile('R6/NAMESPACE'));
  assert.throws(() => session.eval_checked('library(R6)'), /missing export/);
  assert.equal(session.eval_checked('"R6" %in% loadedNamespaces()'), '[1] FALSE\n');
  for (const source of sources) session.import_file('packages/' + source, readArchiveFile(source));
  const script = readFileSync(new URL('fixtures/r6-analysis.R', fixtureRoot), 'utf8');
  result = session.eval_checked(script);
  assert.equal(result, 'Loaded R6 2.6.1; analyzed observations with mean 7\n');
  assert.equal(session.eval_checked('find.package("R6")'), '[1] "packages/R6"\n');
} finally {
  session.free();
}

const fresh = new WasmRSession();
try {
  fresh.eval_checked('.libPaths("packages")');
  assert.throws(() => fresh.eval_checked('library(R6)'), /no package called 'R6'/);
  fresh.import_file('packages/compiled/DESCRIPTION', Buffer.from(
    'Package: compiled\nVersion: 1.0.0\nNeedsCompilation: yes\n'));
  fresh.import_file('packages/compiled/NAMESPACE', Buffer.from('export(value)\n'));
  fresh.import_file('packages/compiled/R/code.R', Buffer.from('value <- 1\n'));
  assert.throws(() => fresh.eval_checked('library(compiled)'), /NeedsCompilation: yes/);
  assert.equal(fresh.eval_checked('1L + 1L'), '[1] 2\n');
} finally {
  fresh.free();
}

console.log(JSON.stringify({ wasm_sha256: sha256(moduleBytes), output: result }));
