#!/usr/bin/env node
// Public production Wasm filename serialization with independent GNU fixtures.
const fs = require("node:fs");
const path = require("node:path");
if (process.argv.length !== 3)
  throw new Error("usage: node serialization-files-smoke.cjs WASM_PACKAGE");
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), "r_wasm.js"));
const fixtures = path.join(__dirname, "../r-embed/tests/fixtures/gnu-browser-rds");
const left = new WasmRSession();
const right = new WasmRSession();
let checks = 0,
  failures = 0;
const value = "list(x=c(1L,2L,3L),message='café',missing=NA_real_)";
function check(label, session, code) {
  checks++;
  try {
    const actual = session.eval(code);
    if (actual !== "[1] TRUE\n") {
      failures++;
      console.log(JSON.stringify({ label, status: "FAIL", actual, code }));
    } else console.log(JSON.stringify({ label, status: "PASS" }));
  } catch (e) {
    failures++;
    console.log(JSON.stringify({ label, status: "THREW", message: String(e) }));
  }
}
try {
  for (const codec of ["plain", "gzip"]) {
    left.import_file("rds/" + codec + ".rds", fs.readFileSync(path.join(fixtures, codec + ".rds")));
    check("imported GNU " + codec, left, `identical(readRDS('rds/${codec}.rds'),${value})`);
  }
  for (const compress of ["FALSE", "TRUE", "'gzip'", "'bzip2'", "'xz'"]) {
    check(
      "save/read codec " + compress,
      left,
      `{saved<-withVisible(saveRDS(${value},'rds/output.rds',compress=${compress}));is.null(saved$value)&&!saved$visible&&identical(readRDS('rds/output.rds'),${value})}`,
    );
    checks++;
    try {
      const bytes = left.export_file("rds/output.rds");
      right.import_file("received.rds", bytes);
      if (right.eval(`identical(readRDS('received.rds'),${value})`) !== "[1] TRUE\n") {
        failures++;
        console.log("crosssession FAIL " + compress);
      } else console.log("crosssession PASS " + compress);
    } catch (e) {
      failures++;
      console.log("crosssession FAIL " + compress + ": " + String(e));
    }
  }
  left.import_file("saved.rds", Uint8Array.of(7, 8));
  check(
    "oversize and invalid write recover",
    left,
    "{large<-tryCatch({saveRDS(raw(1048576),'saved.rds',compress=FALSE);FALSE},error=function(e)TRUE);invalid<-tryCatch({saveRDS(1L,'../outside.rds',compress=FALSE);FALSE},error=function(e)TRUE);large&&invalid&&identical(1L+1L,2L)}",
  );
  checks++;
  try {
    const bytes = left.export_file("saved.rds");
    if (bytes.length !== 2 || bytes[0] !== 7 || bytes[1] !== 8)
      throw new Error("old bytes changed");
    console.log("preserve existing bytes PASS");
  } catch (e) {
    failures++;
    console.log(String(e));
  }
  console.log(`RDS_BROWSER_COMPLETE checks=${checks} failures=${failures}`);
  process.exitCode = failures ? 1 : 0;
} finally {
  left.close();
  right.close();
}
