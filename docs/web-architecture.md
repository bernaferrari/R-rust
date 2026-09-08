# Web architecture

Android and the Kotlin/Wasm browser workbench share `RSessionBackend` and the
platform-neutral models in `apps/workbench/shared`. Android uses Rust through
UniFFI. The browser defaults to the Rust interpreter through `r-wasm`; add
`?runtime=webr` to use the separate WebR runtime for broader package support.
The runtime label identifies which engine is executing a script.

The Rust browser path is Kotlin UI → Promise bridge → module Worker →
wasm-bindgen → `r-embed::RSession`. Each worker owns one session and queues
requests. Values cross the boundary as owned strings or PNG bytes; no borrowed
SEXP or pointer crosses into JavaScript. Checked evaluation rejects errors;
`eval_string` extracts a typed character scalar without parsing printed output.
Plotting runs ordinary R evaluation, including function and S3 dispatch, before
portable numeric `plot.default` draws on the headless device. This is basic
plot support, not complete GNU R graphics or grid compatibility.

Scripts and history persist in browser storage. R objects live in worker
memory. The Rust worker cannot process an interrupt message while synchronously
executing Wasm, so Stop terminates it and reports that the in-memory session was
reset. The next request creates a fresh session. A future shared-memory cancel
flag can preserve the session; the current UI does not promise that behavior.
Package installation is disabled for Rust. WebR remains an explicit alternative,
with its own R engine, packages, SVG devices, and interrupt behavior.

## Build and verification

Install the pinned Rust toolchain, wasm32-unknown-unknown, wasm-pack 0.15.0,
Java 17, Node 24.15 or later and Yarn classic 1.22.22. Then:

```bash
scripts/wasm_m3_smoke.sh
cd apps/workbench
./gradlew --no-daemon :webApp:checkWasmProductionBundleSize
./gradlew :webApp:wasmJsBrowserDevelopmentRun
```

R nonlocal control flow uses `catch_unwind`, including errors and `return`.
The default aborting wasm32 build cannot implement these semantics. The runtime
build script pins `nightly-2026-08-25`, rebuilds std/panic_unwind, and enables
`-Cpanic=unwind`. It skips wasm-opt to avoid optimizer/EH encoding mismatches.
See the [Rust Wasm target documentation](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)
and [wasm-bindgen unwinding requirements](https://wasm-bindgen.github.io/wasm-bindgen/reference/catch-unwind.html).
Install the pinned toolchain with rust-src before building:

```bash
rustup toolchain install nightly-2026-08-25 --profile minimal --component rust-src --target wasm32-unknown-unknown
```

Recent browsers with Wasm exception handling are required. The constructor
rejects aborting builds rather than allowing a superficially successful arithmetic
smoke to disguise broken error/return handling. Unexpected Rust panics close
the session; a fatal Wasm trap resets the worker. Ordinary R errors retain state.

Gradle builds `r-wasm` through this script with wasm-pack's web target and includes the generated
module and Wasm asset in the served `rust-runtime/` directory. The release gate
checks both the Kotlin UI budget (450 KiB) and the Rust runtime asset budget
(25 MiB). The latter is a ceiling, not a download-size claim. CI executes real
Rust Wasm under Node and builds the production browser bundle.

The Chromium test in `tests/browser/rust-runtime.cjs` exercises the real worker,
UI, errors, nonlocal control flow, plotting, and cancellation/reset.

The Node smoke exercises evaluation, errors, typed strings, continuation
parsing, session isolation, solve/mapply contracts, and PNG rendering. It does
not replace browser worker/UI tests or establish complete R compatibility.
Filesystem, native extensions, package coverage and graphics remain bounded
by the Rust implementation's capability ledger.
