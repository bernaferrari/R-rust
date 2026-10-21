import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const rows = readFileSync(new URL('../../crates/r-embed/tests/fixtures/default-byte-diagnostic-cases.tsv', import.meta.url), 'utf8').trimEnd().split('\n').map(row => row.split('\t'));
assert.equal(rows.length, 13);
function decode(value) {
  let result = '';
  for (let i = 0; i < value.length; i++) {
    if (value[i] !== '\\') { result += value[i]; continue; }
    const escaped = value[++i];
    const replacement = { '\\': '\\', n: '\n', r: '\r', t: '\t' }[escaped];
    assert.notEqual(replacement, undefined);
    result += replacement;
  }
  return result;
}
export function runDefaultByteDiagnostics(session, generation) {
  const utf8 = session.eval_checked('isTRUE(l10n_info()[["UTF-8"]])') === '[1] TRUE\n';
  let checks = 0;
  for (const fields of rows) {
    assert.equal(fields.length, 6);
    let error;
    try { session.eval_checked(fields[1]); } catch (failure) { error = String(failure); }
    const expected = decode(fields[utf8 ? 5 : 4]);
    assert.ok(error && error.includes(expected) && !error.includes('\ufffd'), `${fields[0]} host: ${error}, expected: ${expected}`); checks++;
    assert.equal(session.eval_checked('paste(as.character(charToRaw(geterrmessage())),collapse="")'), `[1] "${fields[utf8 ? 3 : 2]}"\n`, `${fields[0]} raw native buffer`); checks++;
    assert.equal(session.eval_checked('1L+1L'), '[1] 2\n'); checks++;
    console.log(JSON.stringify({ phase: 'default-byte-diagnostic', generation, scenario: fields[0], status: 'pass', checks: 3 }));
  }
  assert.equal(checks, 39);
  return checks;
}
