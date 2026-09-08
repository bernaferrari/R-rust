# RPort Android Compose Sample

Minimal Jetpack Compose sample for the real UniFFI embedding API. The app keeps
two independent `RSession` instances alive, evaluates code, renders PNG plots,
lists and loads a bundled pure-R package, and cancels a long-running eval.

Use this example to learn how to embed R in your own Android app. For a complete
editor, console, project browser and plot history, use the
[R Workbench](../../apps/workbench/). These are independent Gradle projects;
the example uses the Rust/UniFFI API directly, without workbench code.

## What It Demonstrates

- Two Android tabs backed by separate Rust R sessions.
- App-private runtime paths via `configureAndroidPaths(...)`.
- Typed asynchronous eval results through `evalAsync(...)` and `takeResult(...)`.
- PNG plot rendering through `renderAsync(...)` and `takeResult(...)`.
- Pure-R package discovery/loading through `installedPackages()` and
  `loadPackage(...)`.
- S3 dispatch from the bundled `androiddemo` package.
- Cooperative cancellation via operation-specific `cancelOperation(...)`.

Callbacks carry the operation ID returned by `evalAsync` or `renderAsync`. The
sample accepts progress only for the active ID and polls terminal results by ID,
so delayed callbacks cannot overwrite a newer operation. Console output comes
from the completed result, including fast operations that finish before their
ID reaches the caller. UI state transitions run on the main dispatcher;
blocking runtime calls run on the IO dispatcher.

## Prepare Bindings

Generate Kotlin bindings and build the Android native library from the repo
root:

```bash
scripts/generate_uniffi_bindings.sh --out-dir examples/android-compose/app/generated
cargo ndk -t arm64-v8a -o examples/android-compose/app/src/main/jniLibs build -p r-uniffi --release
```

The checked-in `crates/r-uniffi/uniffi.toml` sets the Kotlin package to
`com.rport.uniffi`, which matches the sample imports.

For a versioned local release layout instead of editing the sample tree
directly, run:

```bash
scripts/package_release_artifacts.sh --check
```

The generated bundle contains Kotlin bindings under `bindings/kotlin/` and the
Android shared library at `android/jniLibs/arm64-v8a/libr_uniffi.so`.

## Build And Run

From the repository root, after preparing bindings and the native library:

```bash
examples/android-compose/gradlew -p examples/android-compose :app:assembleDebug
adb install -r examples/android-compose/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -W -n com.rport.sample/.MainActivity
```

The example has its own checksum-pinned Gradle wrapper. Use JDK 17 and an Android
SDK with API 35 installed. You can also open this directory directly in Android
Studio.

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
