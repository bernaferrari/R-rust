# R Studio Mobile for Android

R Studio style mobile application built with Jetpack Compose Material 3 and the
Rust/UniFFI R runtime.

## Features Implemented

✅ **Script Editor** with R syntax highlighting and real runtime execution
✅ **Console Output** with ANSI color support and typed result summary
✅ **Plot View** backed by Rust-rendered PNG plots with pinch zoom and pan gestures
✅ **Environment Browser** backed by `ls(all.names = TRUE)` and typed value summaries
✅ **CSV Import** through Android's document picker into app-private storage
✅ **File Browser** showing imported workspace files
✅ **Help / Documentation Viewer**
✅ **Tab Navigation** for all panes
✅ **Adaptive Layout** for phones and tablets
✅ Full keyboard support and shortcuts
✅ Material 3 design system
✅ Dark/Light theme support
✅ Touch optimized interactions

## Project Structure

```
app/src/main/java/com/rstudio/mobile/
├── MainActivity.kt          # Entry point
├── ui/
│   ├── RStudioApp.kt        # Main app layout
│   └── Theme.kt             # Material 3 theme
├── components/
│   ├── ScriptEditor.kt      # R code editor
│   ├── ConsoleView.kt       # Console output
│   ├── PlotView.kt          # Plot renderer with zoom
│   ├── EnvironmentBrowser.kt
│   ├── FileBrowser.kt
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

- **Phone:** Single horizontal pager with bottom tab navigation
- **Tablet:** Split pane layout (editor + console on left, tabs on right)
- All components follow R Studio desktop visual style and behaviour
- 48dp minimum touch targets for all interactive elements
- Proper keyboard insets handling
- Edge-to-edge display support

## Current Runtime Scope

The debug app packages the generated Kotlin UniFFI binding and the arm64-v8a
`libr_uniffi.so`. It can run R code, import CSV files selected from Android
Files, inspect allocated variables, and render supported base `plot(...)`
calls. Broader package installation, native CRAN extensions, and arbitrary
RStudio IDE workflows are still outside this mobile MVP.

## License

MIT
