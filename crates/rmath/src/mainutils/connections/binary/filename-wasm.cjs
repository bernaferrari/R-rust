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
    for (const [operation, message, remaining] of [
        ["writeBin(1L,'binary/output.bin',endian='bad')", "invalid 'endian' argument", 2],
        ["writeBin(list(1L),'binary/output.bin')", "can only write vector objects", 2],
        ["writeBin(structure(1L,x=2L),'binary/output.bin')", "can only write vector objects", 2],
        ["writeBin(1L,'binary/output.bin',size=3L)", "size 3 is unknown on this machine", 0],
        ["writeBin(as.raw(1),'binary/output.bin',size=2L)", "size changing is not supported for raw vectors", 0],
        ["writeBin(1+2i,'binary/output.bin',size=8L)", "size changing is not supported for complex vectors", 0],
        ["writeBin(1L,'binary/output.bin',useBytes=NA)", "invalid 'useBytes' argument", 0],
        ["(function(){old<-options(warn=2);on.exit(options(old));writeBin(1L,'binary/output.bin',size='x',useBytes=NA)})()", "(converted from warning) NAs introduced by coercion", 0],
        ["(function(){old<-options(warn=2);on.exit(options(old));writeBin('a','binary/output.bin',size='x')})()", "(converted from warning) NAs introduced by coercion", 0],
    ]) {
        left.import_file('binary/output.bin', Uint8Array.of(7,8));
        check('write validation: '+operation, left, `identical(tryCatch(${operation},error=function(e){gc();conditionMessage(e)}),"${message}")`);
        checks += 1;
        try { assert.equal(left.export_file('binary/output.bin').length, remaining, operation); }
        catch (error) { failures += 1; console.error(error.message); }
    }
    check('empty write bypasses size validation', left, "{writeBin(integer(),'binary/output.bin',size=3L);identical(readBin('binary/output.bin','raw',n=8L),raw())}");
    check('character write ignores size', left, "{writeBin('a','binary/output.bin',size=3L);identical(readBin('binary/output.bin','raw',n=8L),as.raw(c(97,0)))}");
    check('raw sink reports size error', left, "identical(tryCatch(writeBin(1L,raw(),size=3L),error=function(e)conditionMessage(e)), 'size 3 is unknown on this machine')");
    check('explicit append sink keeps existing bytes on size error', left, "{writeBin(as.raw(c(7,8)),'binary/output.bin');con<-file('binary/output.bin','ab');message<-tryCatch(writeBin(1L,con,size=3L),error=function(e)conditionMessage(e));close(con);identical(message,'size 3 is unknown on this machine')&&identical(readBin('binary/output.bin','raw',n=8L),as.raw(c(7,8)))}");
    check('named text source rejects binary IO', left, "{con<-file('binary/output.bin','w');message<-tryCatch(readBin(con,'raw',n=1L),error=function(e)conditionMessage(e));close(con);identical(message,'can only read from a binary connection')}");
    check('named text sink rejects binary IO before invalid size', left, "{con<-file('binary/output.bin','w');message<-tryCatch(writeBin(1L,con,size=3L),error=function(e)conditionMessage(e));close(con);identical(message,'can only write to a binary connection')}");
    check('raw sink remains visible', left, "{x<-withVisible(writeBin(as.raw(c(1,2)),raw()));x$visible&&identical(x$value,as.raw(c(1,2)))}");
    check('character records keep the next connection record and zero reads preserve position', left, "{writeBin(c('a','b'),'binary/output.bin');con<-file('binary/output.bin','rb');zero<-readBin(con,'character',n=0L);first<-readBin(con,'character',n=1L);second<-readBin(con,'character',n=1L);last<-readBin(con,'character',n=1L);close(con);identical(zero,character())&&identical(first,'a')&&identical(second,'b')&&identical(last,character())}");
    console.log(`BINARY_FILENAME_WASM_COMPLETE checks=${checks} failures=${failures}`);
    if (failures) process.exitCode = 1;
} finally { left.close(); right.close(); }
