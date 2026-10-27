// Exercise a large TRE pattern and same-session recovery in a web-target package.
// Usage: node crates/r-wasm/regex-stack-limit-smoke.mjs /path/to/web-pkg
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const packageDir = path.resolve(process.argv[2] ?? 'crates/r-wasm/pkg');
const wasm = await import(pathToFileURL(path.join(packageDir, 'r_wasm.js')).href);
wasm.initSync({ module: readFileSync(path.join(packageDir, 'r_wasm_bg.wasm')) });
const session = new wasm.WasmRSession();
session.eval_checked("pattern <- strrep('a', 250000L); invisible(NULL)");
assert.throws(
    () => session.eval_checked("grepl(pattern, 'a')"),
    /regex workspace exceeds session memory limit/,
);
assert.equal(
    session.eval_checked("rm(pattern); invisible(gc()); identical(grepl('a', 'a'), TRUE)").trim(),
    '[1] TRUE',
);
session.close();
console.log('Large regex memory denial and same-session recovery passed.');
