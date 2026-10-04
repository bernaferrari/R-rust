#!/usr/bin/env node
// Execute the original dataset contracts through a fresh real Wasm facade.
// Node reads only the independent fixtures; the R runtime has no host library.
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

if (process.argv.length !== 3) throw new Error('usage: node wasm_contract.cjs WASM_PACKAGE_DIRECTORY');
const assets = path.join(__dirname, 'assets');
const inventory = JSON.parse(fs.readFileSync(path.join(assets, 'inventory.json'), 'utf8'));
for (const [file, expected] of Object.entries(inventory.artifacts_sha256)) {
    const actual = crypto.createHash('sha256').update(fs.readFileSync(path.join(assets, file))).digest('hex');
    assert.equal(actual, expected, file);
}
assert.equal(inventory.objects.length, 108);
assert.equal(Object.keys(inventory.topics).length, 91);
assert.equal(inventory.index.length, 108);
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const session = new WasmRSession();
let checks = 0;
function check(label, code) {
    assert.equal(session.eval(code), '[1] TRUE\n', label);
    checks += 1;
}
const quote = JSON.stringify;
try {
    check('no installed host library', "length(.libPaths())==0L&&'package:datasets'%in%search()");
    check('literal promise and namespace separation', `{
        lazy<-getNamespaceInfo('datasets','lazydata');attached<-as.environment('package:datasets')
        code<-quote(lazyLoadDBfetch(KEY,datafile,compressed,envhook));code[[2L]]<-c(59685L,1102L)
        before<-substitute(mtcars,attached);value<-datasets::mtcars
        identical(before,code)&&identical(substitute(mtcars,attached),code)&&
        identical(value,get('mtcars',lazy))&&length(getNamespaceExports('datasets'))==0L&&
        identical(tryCatch(datasets:::mtcars,error=function(e)conditionMessage(e)),"object 'mtcars' not found")
    }`);
    const contracts = fs.readFileSync(path.join(assets, 'object-contracts.tsv'), 'utf8').trimEnd().split('\n').slice(1);
    assert.equal(contracts.length, 108);
    for (const row of contracts) {
        const [name, ...expected] = row.split('\t');
        check(`original type and attributes ${name}`, `{
            v<-get(${quote(name)},lazy,inherits=FALSE)
            identical(paste(typeof(v),length(v),paste(class(v),collapse=','),paste(dim(v),collapse=','),
            paste(names(attributes(v)),collapse=','),sep='\\t'),${quote(expected.join('\t'))})
        }`);
    }
    for (const [topic, names] of Object.entries(inventory.topics)) {
        const members = `c(${names.map(quote).join(',')})`;
        check(`original topic ${topic}`, `{
            target<-new.env();loaded<-withVisible(data(${quote(topic)},package='datasets',envir=target))
            identical(loaded$value,${quote(topic)})&&!loaded$visible&&
            identical(sort(ls(target)),sort(${members}))&&
            all(vapply(${members},function(name)identical(get(name,target),get(name,lazy)),logical(1L)))
        }`);
    }
    check('index shape and exact title', `{
        index<-data(package='datasets')$results
        identical(dim(index),c(108L,4L))&&identical(colnames(index),c('Package','LibPath','Item','Title'))&&
        identical(index[3L,c('Package','Item','Title')],
          c(Package='datasets',Item='BJsales.lead (BJsales)',Title='Sales Data with Leading Indicator'))
    }`);
    check('copy on write and retained promise syntax', `{
        copy<-datasets::mtcars;copy$mpg[[1L]]<--99;gc()
        identical(datasets::mtcars$mpg[[1L]],21)&&identical(substitute(mtcars,attached),code)
    }`);
    // Reuse the same independently pinned 32-value/names program as the native
    // public gate, so this target cannot silently weaken its numeric oracle.
    const native = fs.readFileSync(path.resolve(__dirname, '../../../../r-embed/tests/portable_datasets.rs'), 'utf8');
    const model = native.split('fn portable_datasets_real_mtcars_model_and_covratio_match_gnu()')[1];
    assert.ok(model, 'durable native model contract');
    const program = model.match(/r#"([\s\S]*?)"#/);
    assert.ok(program, 'original model R program');
    check('all 32 covratio values and names', program[1]);
    const expected = fs.readFileSync(path.join(assets, 'values.rds'));
    session.import_file('original-values.rds', expected);
    check('all 592931 original graph bytes', `{
        input<-file('original-values.rds','rb')
        expected<-readBin(input,'raw',n=${expected.length}L);close(input)
        identical(serialize(mget(sort(ls(lazy)),lazy,inherits=FALSE),NULL,version=2),expected)
    }`);
    console.log(`Portable datasets Wasm complete: ${checks} checks;108 objects;91 topics;32 covratio values;${expected.length} exact graph bytes`);
} finally {
    session.close();
}
