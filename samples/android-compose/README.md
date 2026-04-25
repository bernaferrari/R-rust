# RPort Android Compose Sample

Minimal Jetpack Compose sample for the real UniFFI embedding API. The app keeps
two independent `RSession` instances alive, evaluates code, renders PNG plots,
lists and loads a bundled pure-R package, and cancels a long-running eval.

## What It Demonstrates

- Two Android tabs backed by separate Rust R sessions.
- App-private runtime paths via `configureAndroidPaths(...)`.
- Typed eval results through `evalResult(...)`.
- PNG plot rendering through `render(...)`.
- Pure-R package discovery/loading through `installedPackages()` and
  `loadPackage(...)`.
- S3 dispatch from the bundled `androiddemo` package.
- Cooperative cancellation via `cancelCurrentOperation()`.

## Prepare Bindings

Generate Kotlin bindings and build the Android native library from the repo
root:

```bash
scripts/generate_uniffi_bindings.sh --out-dir samples/android-compose/app/generated
cargo ndk -t arm64-v8a -o samples/android-compose/app/src/main/jniLibs build -p r-uniffi --release
```

The checked-in `crates/r-uniffi/uniffi.toml` sets the Kotlin package to
`com.rport.uniffi`, which matches the sample imports.

## Build And Run

```bash
rstudio-mobile/gradlew -p samples/android-compose :app:assembleDebug
adb install -r samples/android-compose/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -W -n com.rport.sample/.MainActivity
```

Use the `Showcase` action first. It loads `androiddemo`, runs an S3 method in
Session A, proves Session B has separate state, and renders labeled line/point
plots.

## Reproducible Showcase Artifacts

The host-side artifact script exercises the same runtime capabilities without an
emulator and writes a transcript plus plot PNGs:

```bash
scripts/android_showcase_artifacts.sh --check
```

Generated files:

- `target/android-showcase/showcase-transcript.txt`
- `target/android-showcase/line-plot.png`
- `target/android-showcase/point-plot.png`

## Demo Plot Scripts

```r
plot(c(1, 2, 3, 4), c(1, 4, 9, 16), type = "l", col = "blue", lwd = 2,
     main = "Android growth", xlab = "x", ylab = "x^2")
plot(c(1, 2, 3, 4), c(3, 1, 4, 2), type = "b", col = "green", cex = 1.3,
     main = "Android points", xlab = "sample", ylab = "value")
```
