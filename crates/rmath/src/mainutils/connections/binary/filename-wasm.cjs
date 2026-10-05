#!/usr/bin/env node
// Real Wasm public facade and file store; no replacement browser backend.
const assert = require('node:assert/strict');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('usage: node filename-wasm.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const left = new WasmRSession();
const right = new WasmRSession();
let failures = 0;
let checks = 0;
function check(label, session, source) {
    checks += 1;
    try { assert.equal(session.eval(source), '[1] TRUE\n', label); }
    catch (error) { failures += 1; console.error(label + ': ' + error.message); }
}
try {
    left.import_file('binary/input.bin', Uint8Array.of(0,1,127,128,255,0));
    right.import_file('binary/input.bin', Uint8Array.of(3,4));
    check('explicit connection control', left, "{con<-file('binary/input.bin','rb');x<-readBin(con,'raw',n=6L);close(con);identical(x,as.raw(c(0,1,127,128,255,0)))}");
    check('filename read', left, "identical(readBin('binary/input.bin','raw',n=6L),as.raw(c(0,1,127,128,255,0)))");
    check('typed endian read', left, "identical(readBin('binary/input.bin','integer',n=3L,size=2L,signed=FALSE,endian='little'),c(256L,32895L,255L))");
    check('raw source control', left, "identical(readBin(as.raw(c(4,5,6)),'raw',n=2L),as.raw(c(4,5)))");
    check('malformed descriptions', left, "identical(tryCatch(readBin(character(),'raw'),error=function(e)conditionMessage(e)),\"invalid 'description' argument\")&&identical(tryCatch(readBin(NA_character_,'raw'),error=function(e)conditionMessage(e)),\"invalid 'description' argument\")");
    check('session isolation', right, "identical(readBin('binary/input.bin','raw',n=8L),as.raw(c(3,4)))");
    check('filename invisible write', left, "{x<-withVisible(writeBin(as.raw(c(0,1,255)),'binary/output.bin'));is.null(x$value)&&!x$visible}");
    check('written typed bytes', left, "{writeBin(c(1L,256L),'binary/output.bin',size=2L,endian='big');identical(readBin('binary/output.bin','raw',n=4L),as.raw(c(0,1,1,0)))}");
    check('missing file and recovery', left, "{failed<-tryCatch(readBin('binary/missing.bin','raw',n=1L),error=function(e)TRUE);identical(failed,TRUE)&&identical(1L+1L,2L)}");
    console.log(`BINARY_FILENAME_WASM_COMPLETE checks=${checks} failures=${failures}`);
    if (failures) process.exitCode = 1;
} finally { left.close(); right.close(); }
