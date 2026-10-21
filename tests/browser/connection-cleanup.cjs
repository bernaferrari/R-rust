#!/usr/bin/env node
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node connection-cleanup.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const cases = [
  "local({ f<-tempfile(); capture.output(print(1),file=f); capture.output(print(2),file=f,append=TRUE); identical(readLines(f),c('[1] 1','[1] 2')) })",
  "local({ f<-tempfile(); con<-file(f); before<-isOpen(con); capture.output(cat('hello'),file=con); closed<-tryCatch({isOpen(con);FALSE},error=function(e)TRUE); !before&&closed&&identical(readLines(f),'hello') })",
  "local({ f<-tempfile(); con<-file(f); tryCatch(capture.output({cat('before');stop('boom')},file=con),error=function(e)NULL); closed<-tryCatch({isOpen(con);FALSE},error=function(e)TRUE); closed&&identical(readLines(f,warn=FALSE),'before')&&identical(capture.output(cat('after')),'after') })",
  "local({ f<-tempfile(); con<-file(f); writeLines('line',con); written<-!isOpen(con); got<-readLines(con); written&&!isOpen(con)&&identical(got,'line') })",
];
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  try {
    for (const source of cases) assert.equal(session.eval_checked(source), '[1] TRUE\n');
    assert.equal(session.eval_checked('1L + 1L'), '[1] 2\n');
  } finally { session.close(); session.free(); }
}
console.log('CONNECTION_CLEANUP_WASM_COMPLETE checks=10 failures=0');
