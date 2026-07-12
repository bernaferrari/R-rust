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
