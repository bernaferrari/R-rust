#!/usr/bin/env node
// Production Wasm facade and per-session connection tables, with no host file.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('usage: node anonymous-wasm.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const left = new WasmRSession();
const right = new WasmRSession();
let checks = 0;
function check(session, source) {
    assert.equal(session.eval(source), '[1] TRUE\n', source);
    checks += 1;
}
try {
    const contract = fs.readFileSync(path.join(__dirname, 'anonymous-contract.R'), 'utf8');
    check(left, `{${contract}\n}`);
    check(left, "{connection<-file('', 'w+b');writeBin(as.raw(c(7,8)),connection);TRUE}");
    check(right, "{connection<-file('', 'w+b');writeBin(as.raw(3),connection);identical(readBin(connection,'raw',n=8L),as.raw(3))}");
    check(left, "{seek(connection,1048577,rw='write');message<-tryCatch(writeBin(as.raw(9),connection),error=function(e){gc();conditionMessage(e)});seek(connection,0,rw='read');identical(message,'anonymous file exceeds 1048576 byte limit')&&identical(readBin(connection,'raw',n=8L),as.raw(c(7,8)))}");
    check(left, "{close(connection);failed<-tryCatch(readBin(connection,'raw',n=1L),error=function(e)TRUE);identical(failed,TRUE)&&identical(1L+1L,2L)}");
    check(right, "{seek(connection,0,rw='read');identical(readBin(connection,'raw',n=8L),as.raw(3))}");
    check(right, "{close(connection);TRUE}");
    assert.deepEqual(left.list_files(), []);
    assert.deepEqual(right.list_files(), []);
    checks += 2;
    console.log(`ANONYMOUS_FILE_WASM_COMPLETE checks=${checks} failures=0`);
} finally { left.close(); right.close(); }
