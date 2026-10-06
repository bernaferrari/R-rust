// Diagnostic sampling only. Public browser tests and all runtime budgets stay
// unchanged; a named diagnostic module must match production execution bytes.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {Session} from 'node:inspector';
import {resolve, sep} from 'node:path';
import {performance} from 'node:perf_hooks';
import {fileURLToPath, pathToFileURL} from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));
const packageDirectory = resolve(process.argv[2]);
const evidence = resolve(process.argv[3]);
const modulePath = process.argv[4] || resolve(packageDirectory, 'r_wasm_bg.wasm');
mkdirSync(evidence, {recursive:true});
const moduleBytes = readFileSync(modulePath);
const optimization = JSON.parse(readFileSync(resolve(packageDirectory,'rust-runtime-optimization.json'),'utf8'));
if (process.argv[4]) assert.equal(createHash('sha256').update(moduleBytes).digest('hex'), optimization.profile_names.sha256);
console.log(JSON.stringify({phase:'profile', production:optimization.output, diagnostic_module_sha256:createHash('sha256').update(moduleBytes).digest('hex'), names:optimization.profile_names, node:process.version, platform:process.platform}));
const {WasmRSession, initSync} = await import(new URL('r_wasm.js',pathToFileURL(packageDirectory+sep)).href);
const inspector = new Session();
inspector.connect();
const post = (method, params={}) => new Promise((resolve,reject)=>inspector.post(method,params,(error,result)=>error?reject(error):resolve(result)));
await post('Profiler.enable');
await post('Profiler.setSamplingInterval',{interval:1000});
function samplingSummary(profile) {
 const nodes=new Map(profile.nodes.map(node=>[node.id,node]));
 const parents=new Map();for(const node of profile.nodes)for(const child of node.children||[])parents.set(child,node.id);
 const categories={},functions=new Map();
 for(let i=0;i<(profile.samples||[]).length;i++) {
  const id=profile.samples[i], ms=(profile.timeDeltas[i]||0)/1000;
  const name=nodes.get(id)?.callFrame.functionName||'(unknown)';functions.set(name,(functions.get(name)||0)+ms);
  const names=[];for(let current=id;current!==undefined;current=parents.get(current))names.push(nodes.get(current)?.callFrame.functionName||'');
  const stack=names.join('\n').replace(/\[[0-9a-f]+\]/g,'');
  const category=/sexp::gc|sexp::gengc|drain_mark_stack|drain_trace_worklist|collect_garbage|sweep_nodes|GCState/.test(stack)?'collection':
   /png::encoder|encode_png|AndroidHeadlessRenderer.*try_finish/.test(stack)?'PNG encoding':
   /Scene.*replay|AndroidHeadlessRenderer|renderplot::/.test(stack)?'drawing':
   /RArena.*alloc|Rf_allocVector|alloc::alloc|NodeFactory.*allocate/.test(stack)?'allocation':'evaluation/other';
  categories[category]=(categories[category]||0)+ms;
 }
 return {scope:'Sampled CPU milliseconds, classified by stack names; not phase wall-clock durations. Raw profiles retained.',categories,top_self_sample_ms:[...functions].sort((a,b)=>b[1]-a[1]).slice(0,15)};
}
let index = 0;
async function measure(phase, action) {
  await post('Profiler.start');
  const start=performance.now(), cpu=process.cpuUsage();
  let result, error;
  try { result=action(); } catch(e) { error=String(e); }
  const elapsed=performance.now()-start, usage=process.cpuUsage(cpu);
  const {profile}=await post('Profiler.stop');
  const file=`${String(index++).padStart(2,'0')}-${phase}.cpuprofile`;
  writeFileSync(resolve(evidence,file),JSON.stringify(profile));
  console.log(JSON.stringify({phase,ms:elapsed,cpu_us:usage,state:error?'error':'finished',error,result,profile:file,sampling:samplingSummary(profile)}));
}
await measure('instantiate',()=>{initSync({module:moduleBytes});return true;});
const bytes=readFileSync(resolve(root,'crates/r-embed/tests/fixtures/gnu-bytecode-calls/identity-promise.rds'));
const stream=Buffer.alloc(16);[12,20,0,1].forEach((word,i)=>stream.writeInt32BE(word,i*4));
const offset=bytes.indexOf(stream);assert.ok(offset>=0);bytes.writeInt32BE(16,offset+4);
const methods=[
 `f<-unserialize(as.raw(c(${Array.from(bytes).join(',')})));identical(f(41L),quote(x))`,
 "g<-unserialize(serialize(f,NULL));identical(g(99L),quote(x))",
 "invisible(capture.output(d<-compiler::disassemble(f)));identical(d[[2]][[2]],as.name('GETFUN.OP'))&&identical(d[[3]][[3]][[2]][[2]],as.name('LDCONST.OP'))",
 "local({setClass('SelectA');setClass('SelectB',contains='SelectA');setGeneric('selectprobe',function(x)standardGeneric('selectprobe'));setMethod('selectprobe','SelectA',function(x)42L);m<-selectMethod('selectprobe','SelectB');identical(m(new('SelectB')),42L)&&identical(as.character(m@defined),'SelectA')})",
 "local({setClass('A');setClass('B',contains='A');setClass('C',contains='B');setClass('D',contains=c('A','C'));setGeneric('f',function(x)standardGeneric('f'));setMethod('f','C',function(x)'C');setMethod('f','A',function(x)'A');identical(f(new('D')),'C')})",
 "identical(Re(fft(c(1,0,0,0))),rep(1,4))",
];
let session;
await measure('methods-initialize',()=>{session=new WasmRSession();return true;});
if(session)try {
 for(let phase=0;phase<methods.length;phase++)await measure(`methods-${phase}`,()=>{const output=session.eval_checked(methods[phase]);assert.ok(output.startsWith('[1] TRUE\n'));return output;});
 await measure('methods-final-gc-output',()=>session.eval_checked('gc()'));
}finally{session.close();session.free();}
const examples=readFileSync(resolve(root,'website/src/data/examples.ts'),'utf8');
const match=examples.match(/id: "sunflower"[\s\S]*?code: `([^`]+)`/);assert.ok(match,'Original sunflower example must be present');
const sunflower=match[1];
console.log(JSON.stringify({phase:'sunflower-source',sha256:createHash('sha256').update(sunflower).digest('hex'),bytes:Buffer.byteLength(sunflower),width:800,height:600,source:'website/src/data/examples.ts'}));
for (const flow of ['direct','after-previous-plot']) {
 session=undefined;
 await measure(`sunflower-${flow}-initialize`,()=>{session=new WasmRSession();return true;});
 if(!session)continue;
 try {
  const render=(code)=>{const output=session.eval_interactive(code,800,600);try{assert.equal(output.has_error(),false,output.error());assert.equal(output.has_png(),true);const png=output.png();assert.deepEqual(Array.from(png.subarray(0,8)),[137,80,78,71,13,10,26,10]);return{output:output.output(),png_bytes:png.length};}finally{output.free();}};
  if(flow==='after-previous-plot')await measure(`sunflower-${flow}-previous`,()=>render('plot(1:3,3:1)'));
  await measure(`sunflower-${flow}-cold`,()=>render(sunflower));
  await measure(`sunflower-${flow}-warm`,()=>render(sunflower));
  await measure(`sunflower-${flow}-final-gc-output`,()=>session.eval_checked('gc()'));
 }finally{session.close();session.free();}
}
inspector.disconnect();
