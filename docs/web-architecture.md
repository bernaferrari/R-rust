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

The browser shell deliberately reports `canExecuteR = false` until the
full `r-embed` interpreter backend is made Wasm-compatible. This prevents a
misleading partial evaluator from silently producing incorrect results. The
next backend work is isolated behind `RSessionBackend`; it does not require
changing the Android UI or native session.
