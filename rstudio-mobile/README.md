# R Studio Mobile for Android

Complete R Studio style mobile application built with Jetpack Compose Material 3.

## Features Implemented

✅ **Script Editor** with full R syntax highlighting
✅ **Console Output** with ANSI color support
✅ **Plot View** with pinch zoom and pan gestures
✅ **Environment Browser**
✅ **File Browser**
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

## License

MIT
