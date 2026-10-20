#!/usr/bin/env node
// Execute imported GNU streams through the production Wasm facade. Mutated
// instructions must take effect even though the serialized source is retained.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
if (process.argv.length !== 3) throw new Error('Usage: node gnu-opcode-contracts.cjs WASM_PACKAGE');
const { WasmRSession } = require(path.join(path.resolve(process.argv[2]), 'r_wasm.js'));
const fixtures = path.join(__dirname, '../../crates/r-embed/tests/fixtures');
const names = fs.readFileSync(path.join(fixtures, 'gnu-bytecode-getter-call/names.rds'));
const attr = fs.readFileSync(path.join(fixtures, 'gnu-bytecode-getter-call/attr.rds'));
const matrix = fs.readFileSync(path.join(fixtures, 'gnu-bytecode-matsubset2/matsubset2.rds'));
const array = fs.readFileSync(path.join(fixtures, 'gnu-bytecode-subset2-n/subset2-n-const.rds'));
const variable = fs.readFileSync(path.join(fixtures, 'gnu-bytecode-subset2-n/subset2-n.rds'));
const namesWords = [12,20,1,61,2,23,4,35,99,6,100,105,7,18,16,9,86,7,23,10,35,98,12,13,62,2,4,20,2,1];
const attrWords = [12,20,1,61,2,23,4,35,34,6,99,7,100,105,8,20,16,10,86,8,23,11,35,34,6,98,13,14,62,2,4,20,2,1];
const matrixWords = [12,20,1,110,0,12,16,2,16,2,107,0,1];
const arrayWords = [12,20,1,110,0,15,16,2,16,3,16,4,113,0,3,1];
function change(bytes, words, replacements) {
  const stream = Buffer.alloc(words.length * 4);
  words.forEach((word, i) => stream.writeInt32BE(word, i * 4));
  const offset = bytes.indexOf(stream);
  assert.ok(offset >= 0, 'fixture contains the expected opcode stream');
  assert.equal(bytes.indexOf(stream, offset + 1), -1, 'opcode stream is unique');
  const result = Buffer.from(bytes);
  for (const [index, word] of replacements) result.writeInt32BE(word, offset + index * 4);
  return result;
}
function malformed(bytes, words, first) {
  return change(bytes, words, words.map((_, i) => [i, first[i] ?? 1]));
}
let checks = 0;
for (let generation = 0; generation < 2; generation++) {
  const session = new WasmRSession();
  const load = bytes => session.eval_checked(`f <- unserialize(as.raw(c(${[...bytes].join(',')})))`);
  const yes = source => { assert.equal(session.eval_checked(source), '[1] TRUE\n'); checks++; };
  const fails = (bytes, call) => {
    assert.throws(() => { load(bytes); session.eval_checked(call); });
    assert.equal(session.eval_checked('1L + 1L'), '[1] 2\n');
    checks++;
  };
  try {
    load(names);
    yes("identical(withVisible(f(c(a=1,b=2), 'z')), list(value=c(z=1,b=2), visible=TRUE))");
    yes("x<-c(a=1,b=2); y<-x; r<-f(x,'z'); identical(names(y),c('a','b')) && identical(names(r),c('z','b'))");
    yes("e<-new.env(parent=baseenv()); e$names<-function(x)c('u','v'); e[['names<-']]<-function(x,value){attr(x,'marked')<-value;x}; environment(f)<-e; identical(attr(f(1:2,'ok'),'marked'),c('ok','v'))");
    load(attr);
    yes("identical(attr(f(structure(1:3,a=c('p','q','r')),'z'),'a'),c('z','q','r'))");
    load(matrix);
    yes("identical(f(matrix(1:6,2,3)),1L) && {`[[.foo`<-function(x,i,j)paste0(i,j); identical(f(structure(matrix(1:4,2,2),class='foo')),'11')}");
    load(array);
    yes("identical(f(array(1:24,c(2,3,4))),15L)");
    load(variable);
    yes("identical(f(array(1:24,c(2,3,4)),2L,3L,4L),24L)");
    fails(change(names,namesWords,[[6,2]]), "f(c(a=1,b=2),'z')");
    fails(change(attr,attrWords,[[6,2]]), "f(structure(1:3,a=c('p','q','r')),'z')");
    fails(change(matrix,matrixWords,[[7,1]]), 'f(matrix(1:6,2,3))');
    load(change(array,arrayWords,[[11,3]]));
    yes('identical(f(array(1:24,c(2,3,4))),9L)');
    fails(malformed(names,namesWords,[12,99,0]), "f(c(a=1,b=2),'z')");
    fails(malformed(matrix,matrixWords,[12,107,0]), 'f(matrix(1:6,2,3))');
    fails(malformed(array,arrayWords,[12,113,0,3]), 'f(array(1:24,c(2,3,4)))');
  } finally { session.close(); session.free(); }
}
assert.equal(checks, 28);
console.log(`GNU_OPCODE_WASM_COMPLETE checks=${checks} failures=0`);
