# Plan 011: Make ANSI console rendering safe and theme-accessible

> **Executor instructions**: Work in an isolated worktree based on the reviewed
> Android editor commit. Execute all tests and STOP conditions; a reviewer owns
> the index.

## Status

- **Priority**: P0
- **Effort**: S
- **Risk**: MED
- **Depends on**: Plan 004
- **Category**: crash / accessibility
- **Planned at**: commit `462f1280`, 2026-08-12
- **Tracker**: `rport-jxfp.2.3`

## Why this matters

`AnsiParser` removes escape bytes while applying spans at offsets from the
original input. Styled output can therefore create ranges beyond the rendered
text and crash Compose. It also parses only one integer, so normal sequences
such as `\u001b[1;31m` lose their meaning. The fixed VS Code palette includes
light yellow/green values with inadequate contrast on the app's light surface.

## Current state

- `util/AnsiParser.kt:25-44` calls `addStyle` using `mIndex + 1` and
  `nextEscape`, both indices into the escape-containing source rather than the
  builder's escape-free output.
- `substring(...).toIntOrNull()` rejects compound SGR parameters.
- SGR 0/default, bold, bright colors, malformed/truncated sequences, and
  back-to-back codes lack tests.
- `ConsoleView.kt:108` supplies no theme palette, while the app supports both
  light and dark Material schemes.

## Scope

**In scope**:

- `rstudio-mobile/app/src/main/java/com/rstudio/mobile/util/AnsiParser.kt`
- The ANSI call site in `components/ConsoleView.kt`
- Focused JVM tests under `app/src/test/java/com/rstudio/mobile/util/`

**Out of scope**: terminal cursor control, full ECMA-48 emulation, console
history storage, or editor syntax highlighting.

## Steps

### Step 1: Parse SGR as a state machine over output offsets

Walk the input once. Append only visible text, record span start/end from the
builder/output length, and update an explicit style state at each complete
`ESC[...m`. Support empty/0 reset, 1/22 bold, 30–37/39 foreground, and 90–97
bright foreground. Parse semicolon-separated parameters in order. Unknown or
malformed sequences must never throw; preserve incomplete escape text literally
and ignore complete unsupported SGR parameters.

### Step 2: Inject a theme-aware palette

Make the parser accept a small immutable palette. Construct light and dark
palettes from explicit audited colors at the Compose call site (or a pure
factory keyed by dark/light surface). Default/reset inherits `onSurface` rather
than forcing white. Every non-inherited foreground used on its intended console
surface must meet WCAG 2.2 AA 4.5:1 for normal 13sp text; add pure contrast
assertions for both palettes.

### Step 3: Add adversarial and semantic regressions

Test plain text, one color, compound bold+color, reset/default, adjacent codes,
bright colors, unknown codes, malformed numbers, truncated escapes, Unicode,
and randomized strings containing ESC/`[`/`;`/`m`. Assert visible text exactly,
all span ranges within output bounds, and no exception.

## Verification

```sh
ANDROID_HOME=/Users/bernardoferrari/Library/Android/sdk ./gradlew :app:testDebugUnitTest --no-daemon
ANDROID_HOME=/Users/bernardoferrari/Library/Android/sdk ./gradlew :app:compileDebugKotlin --no-daemon
ANDROID_HOME=/Users/bernardoferrari/Library/Android/sdk ./gradlew :app:lintDebug --no-daemon
```

## Done criteria

- [ ] No parsed span can reference an offset outside escape-free output.
- [ ] Common/compound SGR state and resets render correctly.
- [ ] Malformed/truncated/random input cannot crash.
- [ ] Light and dark foreground palettes meet 4.5:1 on their surfaces.
- [ ] Unit, compile, lint, diff, and scope gates pass.

## STOP conditions

Stop if Compose cannot expose span ranges for deterministic tests, palette
contrast requires changing the global app theme, or the fix expands into a
terminal emulator.

