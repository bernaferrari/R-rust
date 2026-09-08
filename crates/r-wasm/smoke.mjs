// M3 smoke test (docs/web-architecture.md): instantiate the wasm-bindgen
// package, open one session, and assert the oracle string. Run via
// scripts/wasm_m3_smoke.sh, which builds crates/r-wasm with wasm-pack first.
// (wasm-pack --target nodejs emits CommonJS that self-initializes at load,
// so there is no async init() to await — the ESM named import binds after
// the module has fully loaded.)
import { WasmRSession } from './pkg/r_wasm.js';

const s = new WasmRSession();

const out = s.eval("1+1");
if (out !== "[1] 2") throw new Error(`M3 oracle mismatch: ${JSON.stringify(out)}`);

if (!s.is_input_complete("1 + 1")) throw new Error("complete input reported incomplete");
if (s.is_input_complete("f <- function(x) {")) {
    throw new Error("incomplete input reported complete");
}

const names = s.global_binding_names();
s.eval("m3_smoke_var <- 42");
if (!s.global_binding_names().includes("m3_smoke_var")) {
    throw new Error("global_binding_names did not observe a new binding");
}
if (s.global_binding_names().includes("..rport_handles..")) {
    throw new Error("engine-internal handle environment leaked into bindings");
}

s.close();

console.log("M3 oracle ok:", out);

const runtime = new WasmRSession();
if (runtime.eval_string("paste(c('a','b'), collapse='|')") !== 'a|b') throw new Error('typed string bridge');
let rejected = false;
try { runtime.eval_checked("stop('expected failure')"); } catch (_) { rejected = true; }
if (!rejected) throw new Error('errors must reject checked evaluation');
runtime.eval_checked("stopifnot(identical(mapply(function(x)x,1:3),1:3)); a <- matrix(c(1,2,3,4),2); stopifnot(isTRUE(all.equal(solve(a,c(5,6)),c(-1,2))))");
const png = runtime.render_png("plot(x=1:3,y=3:1,col='red')", 100, 100);
if (png[0] !== 137 || png[1] !== 80 || png.length < 100) throw new Error('render bridge');
const isolated = new WasmRSession();
if (isolated.global_binding_names().includes('a')) throw new Error('session isolation');
for (const instance of [runtime, isolated]) { instance.close(); instance.free(); }
s.free();
console.log('Rust WASM owned values, errors, numeric contracts, graphics and isolation passed');
