// Exercise actual Wasm sessions with the same independent GNU/public fixtures
// as native embedding. Accept either a bindgen web or nodejs package directory.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
const directory=path.resolve(process.argv[2] ?? 'crates/r-wasm/pkg');
const runtime=await import(pathToFileURL(path.join(directory,'r_wasm.js')).href);
if (runtime.initSync) runtime.initSync({module:fs.readFileSync(path.join(directory,'r_wasm_bg.wasm'))});
const fixture=fs.readFileSync(new URL('../r-embed/tests/fixtures/loess-isolation.R',import.meta.url),'utf8');
const gnu=fs.readFileSync(new URL('../r-embed/tests/fixtures/gnu-loess-surface.rds',import.meta.url));
let checks=0;
function check(session,code) { assert.equal(session.eval_checked(code).trim(),'[1] TRUE',code);checks++; }
function dispose(session) {session.close();session.free();}
const original=new runtime.WasmRSession();
let bytes;
try {
    check(original,fixture);
    original.eval_checked("x<-seq(0,1,length.out=30);y<-sin(5*x);f<-loess(y~x);q<-c(.15,.45,.75);expected<-predict(f,q);saveRDS(list(model=f,q=q,expected=expected),'model.rds',compress=FALSE)");
    bytes=original.export_file('model.rds');
} finally {dispose(original);}
const restored=new runtime.WasmRSession();
try {
    restored.import_file('model.rds',bytes);
    check(restored,"saved<-readRDS('model.rds');identical(saved$expected,predict(saved$model,saved$q))");
} finally {dispose(restored);}
const independent=new runtime.WasmRSession();
try {
    independent.import_file('gnu.rds',gnu);
    check(independent,"saved<-readRDS('gnu.rds');max(abs(saved$expected-predict(saved$model,saved$q)))<1e-9");
    for(const mutation of ['parameter[1]<-5L','a<-integer(0)','vval<-numeric(0)']) {
        check(independent,`saved<-readRDS('gnu.rds');saved$model$kd$${mutation};inherits(tryCatch(predict(saved$model,saved$q),error=function(e)e),'error')`);
        check(independent,"saved<-readRDS('gnu.rds');max(abs(saved$expected-predict(saved$model,saved$q)))<1e-9");
    }
} finally {dispose(independent);}
console.log(`LOESS isolation: ${checks} checks passed`);
