# Web architecture

The Android app and browser app share the platform-neutral workbench contract in
`rstudio-mobile/shared`:

- `RSessionBackend` is the execution boundary.
- `WorkbenchState` and the table/environment/package models are target-neutral.
- `ReportRenderer` is compiled into both JVM and Wasm artifacts.

The Android target keeps its native Rust/UniFFI backend. The browser target is
`rstudio-mobile/webApp` and builds with Kotlin/Wasm:

```bash
cd rstudio-mobile
./gradlew :webApp:wasmJsDevelopmentExecutableCompileSync
./gradlew :webApp:wasmJsBrowserDevelopmentRun
```

The browser adapter uses WebR, the established R-in-WebAssembly runtime, with
PostMessage worker communication so it does not require cross-origin-isolated
SharedArrayBuffer headers. It evaluates scripts, inspects objects, pages data
frames, searches R topics, lists/loads/installs WebR packages, and renders SVG
plots. The web shell adds tabs, local file import, console history, report
download, and browser persistence. The native Android session is unchanged;
both backends remain behind `RSessionBackend`.

### R-in-WASM milestones M2/M3 (plan)

M2 is a `wasm-bindgen` session boundary over `r-embed::RSession`. The native
session is unchanged, and there is no UniFFI async runtime on wasm:

- Exported methods: `eval(code: &str) -> String` (display output),
  `is_input_complete(code: &str) -> bool` (continuation-prompt probe), and a
  planned `global_binding_names() -> Vec<String>` snapshot of the global
  environment bindings (not yet in `r-embed`; M2 adds it as an owned value,
  never raw `SEXP`).
- Console callbacks replace `R_ReadConsole`/`R_WriteConsole`
  (`rmath-rs/rmath/src/unix/system.rs`): the wasm build wires
  `write_console`/`write_console_ex` to a JS string sink and `read_console`
  to a JS prompt callback (or an EOF stub). The `Rstd_*` defaults in
  `rmath-rs/rmath/src/unix/sys_std.rs` (stdout, history, event loop) stay
  native-only.
- No UniFFI on wasm: the browser boundary is `wasm-bindgen` only.
  `crates/r-uniffi` and its async runtime remain a native/Android surface and
  stay out of the `wasm32-unknown-unknown` build.

M3 is a Node smoke test (shape, not yet wired):

```bash
node --experimental-wasm-bigint --experimental-wasm-bulk-memory smoke.mjs
```

```js
// smoke.mjs (shape): instantiate the wasm-bindgen pkg, open one session,
// and assert the oracle string.
import init, { WasmRSession } from './pkg/r_wasm.js';
await init();
const s = new WasmRSession();
const out = s.eval("1+1");
if (out !== "[1] 2") throw new Error(`M3 oracle mismatch: ${out}`);
if (!s.is_input_complete("1 + 1")) throw new Error("complete input reported incomplete");
if (s.is_input_complete("f <- function(x) {")) throw new Error("incomplete input reported complete");
console.log("M3 oracle ok:", out);
```

The native oracle test `wasm_m3_oracle_shape` in
`crates/r-embed/tests/embed.rs` pins the contract
(`session.eval("1+1") == "[1] 2"`) the future wasm boundary must satisfy.

Remaining code work (WasmAudit steps 7-11, the M2 boundary):

- Step 7, console shims: `rmath-rs/rmath/src/unix/system.rs`
  (`R_ReadConsole`, `R_WriteConsole`, `R_WriteConsoleEx`, callback table),
  `rmath-rs/rmath/src/unix/sys_std.rs` (`Rstd_*` console/history/event
  defaults), `rmath-rs/rmath/src/mainutils/memory_main.rs`
  (`R_ReadConsole_memory`, `R_WriteConsole_memory`),
  `rmath-rs/rmath/src/library/utils/io.rs` (`ConsoleGetcharWithPushBack` and
  the menu-selection stub, both expecting `R_ReadConsole`).
- Step 8, env/startup shims: `rmath-rs/rmath/src/unix/system.rs`
  (`process_system_Renviron`, `process_site_Renviron`,
  `process_user_Renviron`, `R_HomeDir`, `BindDomain`; currently no-op stubs),
  `rmath-rs/rmath/src/mainutils/sysutils.rs` (`R_HomeDir` via
  `env::var("R_HOME")`), `rmath-rs/rmath/src/mainutils/startup.rs`
  (`Rprofile`/`Rprofile.site` `fopen` paths),
  `rmath-rs/rmath/src/mainutils/CommandLineArgs.rs` (`R_GetNoRenviron` and
  related CLI-state accessors).
- Step 9, VFS/file shims: `rmath-rs/rmath/src/mainutils/startup.rs` plus
  `rmath-rs/rmath/src/mainutils/sysutils.rs` (`R_FileExists`, `R_FileMtime`,
  `R_HomeDir`-anchored paths assume a real FS and `R_HOME`); wasm needs an
  in-memory/preloaded-FS policy or clean "no FS" errors instead of libc
  `fopen` assumptions.
- Step 10, dynload stub (reject, never load):
  `rmath-rs/rmath/src/mainutils/rdynload.rs`,
  `rmath-rs/rmath/src/unix/dynload.rs`,
  `rmath-rs/rmath/src/mainutils/registration.rs` (`R_registerRoutines`,
  `dyn.load`/`dyn.unload`/`.Call`/`.C`); wasm returns a clean "native
  extensions unsupported" error, the same policy as the Android sandbox,
  never `dlopen`.
- Step 11, `wasm-bindgen` boundary crate (new, additive): a thin wrapper
  around `r-embed::RSession` exposing `eval` / `is_input_complete` /
  `global_binding_names` plus console-callback registration.
  `scripts/wasm_toolchain_check.sh` currently gates only `rmath`,
  `r-graphics-engine`, and `r-device-android-headless` on
  `wasm32-unknown-unknown`; extending it to the boundary crate is M3 work.
