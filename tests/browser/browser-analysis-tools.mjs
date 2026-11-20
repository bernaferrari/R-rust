import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const directory = process.argv[2];
assert.ok(directory, "Expected the browser-analysis-tools Wasm package directory");
const {initSync, WasmRSession} = await import(pathToFileURL(resolve(directory, 'r_wasm.js')));
const module = readFileSync(resolve(directory, 'r_wasm_bg.wasm'));
initSync({module});
const session = new WasmRSession();
try {
  const output = session.eval_checked(`
    omitted <- c(".check_packages", ".install_packages", ".build_packages",
      ".check_package_CRAN_incoming", "format.check_package_CRAN_incoming",
      "check_packages_in_dir")
    for (name in omitted) {
      fn <- getFromNamespace(name, "tools")
      stopifnot(is.function(fn))
      msg <- tryCatch(fn(), error=conditionMessage)
      stopifnot(grepl("unavailable in the slim browser build", msg, fixed=TRUE))
    }
    stopifnot(identical(tools::file_ext(c("a.csv", "b.R")), c("csv", "R")),
      identical(tools::file_path_sans_ext(c("a.csv","b.R")), c("a","b")))
    cat("6 maintenance functions refuse explicitly; file helpers pass")
  `);
  assert.equal(output, '6 maintenance functions refuse explicitly; file helpers pass');
  const png = session.render_png('plot(1:3)', 600, 440);
  assert.deepEqual(Array.from(png.subarray(0, 8)), [137, 80, 78, 71, 13, 10, 26, 10]);
  session.eval_checked('pdf("out.pdf", width=6, height=4); plot(1:3); invisible(dev.off())');
  assert.equal(Buffer.from(session.export_file('out.pdf')).subarray(0, 5).toString(), '%PDF-');
  console.log(JSON.stringify({
    wasm_sha256: createHash('sha256').update(module).digest('hex'),
    omitted_diagnostics: 6, file_helpers: 'pass', png: 'pass', pdf: 'pass',
  }));
} finally { session.free(); }
