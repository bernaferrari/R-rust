# R Workbench for Android

Project-first mobile R workbench built with Jetpack Compose Material 3 and the
Rust/UniFFI R runtime.

## Features Implemented

✅ **Script Editor** with selection/current-line/full-file execution and hardware-keyboard shortcuts
✅ **Interactive Console** with command history, cancellation, ANSI output, and typed results
✅ **Plot History** backed by Rust-rendered PNG plots with zoom, reset, export, and Android sharing
✅ **Environment Browser** with search and object-to-data-viewer inspection
✅ **Data Import** for CSV, TSV, text tables, RDS, and RData through Android's document picker
✅ **Project Folders** through Android's Storage Access Framework with persistent access and write-back
✅ **Recoverable Drafts** and restored project working directories after process recreation
✅ **Package Browser** for installed pure-R packages
✅ **Help / Documentation Viewer**
✅ Four-destination phone navigation and an adaptive tablet IDE workspace
✅ Full keyboard support and shortcuts
✅ Material 3 design system
✅ Dark/Light theme support
✅ Touch optimized interactions

## Project Structure

```
app/src/main/java/com/rstudio/mobile/
├── MainActivity.kt          # Entry point
├── ui/
│   ├── RStudioApp.kt        # Adaptive workbench shell
│   └── Theme.kt             # Material 3 theme
├── components/
│   ├── ScriptEditor.kt      # R code editor
│   ├── ConsoleView.kt       # Console output
│   ├── PlotView.kt          # Plot renderer with zoom
│   ├── EnvironmentBrowser.kt
│   ├── FileBrowser.kt       # SAF-backed project files
│   ├── PackageBrowser.kt
│   └── HelpViewer.kt
└── util/
    ├── RSyntaxHighlighter.kt
    └── AnsiParser.kt
```

## Building

```bash
cd rstudio-mobile
./gradlew :app:assembleDebug
```

For the release smoke path, run:

```bash
../scripts/android_package_smoke.sh --check
```

## Layout Behaviour

- **Phone:** Editor, Console, Inspect, and Files destinations in a bottom navigation bar
- **Tablet:** Editor + console on the left and inspect/files panes on the right
- The primary Run/Stop action remains available in the app bar
- 48dp minimum touch targets for all interactive elements
- Proper keyboard insets handling
- Edge-to-edge display support

## Current Runtime Scope

Gradle builds the Rust runtime through `cargo-ndk` and packages the generated
Kotlin UniFFI binding and arm64-v8a `libr_uniffi.so`. The APK smoke check fails
if the native runtime is absent. Native CRAN extensions and arbitrary package
installation remain outside the supported scope; the package browser describes
the current pure-R package policy explicitly.

## License

MIT
