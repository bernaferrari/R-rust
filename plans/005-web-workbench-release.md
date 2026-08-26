# Plan 005: Make the Web workbench shippable and accessible

> **Executor instructions**: Follow all steps and browser regressions. Stop on
> a STOP condition. A reviewer maintains the index.
>
> **Drift check**:
> `git diff --stat 462f1280..HEAD -- rstudio-mobile/webApp rstudio-mobile/build.gradle.kts rstudio-mobile/settings.gradle.kts scripts/web_toolchain_check.sh`

## Status

- **Priority**: P0
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `462f1280`, 2026-08-12
- **Trackers**: `rport-jxfp.2.1`, `rport-jxfp.2.2`

## Why this matters

The shipped panel tabs are nonfunctional: assigning `hidden="false"` still
hides an element because `hidden` is boolean, while `.panel { display:grid }`
overrides the browser's default hidden styling. A production Wasm build also
fails resolving Binaryen 120, although the development-only gate passes.
Finally, 40px controls, 14px form text, missing labels, and non-semantic tabs
miss basic phone and assistive-technology requirements.

## Current state

- `webApp/.../Main.kt:276-283` calls `setAttribute("hidden", "false")` for the
  active panel. Browser inspection showed every panel's `.hidden` property true
  after clicking Data.
- `webApp/.../resources/index.html:18` gives buttons a 40px minimum; line 37
  gives every `.panel` `display:grid` without a `[hidden]` exception; lines
  52-54 use 14px input/editor text.
- `Main.kt:204-212` uses placeholders instead of labels for console, object,
  package, and help fields. Panel buttons have no `role=tab`, `aria-selected`,
  or panel relationship.
- `Main.kt:243-248,354` serializes all documents to localStorage on every input
  event.
- `Main.kt:121-125` guards `webR.init()` with a boolean only; environment and
  package refresh launch concurrently at lines 351-352 and can race init.
- `:webApp:wasmJsBrowserDevelopmentWebpack` succeeds. The production task fails
  because settings repositories suppress Kotlin's dynamically added Binaryen
  distribution repository and Maven cannot find
  `com.github.webassembly:binaryen:120`.

## Commands

| Purpose | Command | Expected |
|---|---|---|
| Web tests | `./gradlew :webApp:wasmJsBrowserTest --no-daemon` | exit 0 |
| Development bundle | `../scripts/web_toolchain_check.sh` from `rstudio-mobile/` parent/root as documented | exit 0 |
| Production bundle | `./gradlew :webApp:wasmJsBrowserProductionWebpack --console=plain --no-daemon` | exit 0 |
| Shared tests | `./gradlew :shared:allTests --no-daemon` | exit 0 |

## Scope

**In scope**:

- `rstudio-mobile/webApp/src/wasmJsMain/kotlin/Main.kt`
- `rstudio-mobile/webApp/src/wasmJsMain/resources/index.html`
- New Web/Wasm tests under `rstudio-mobile/webApp/src/wasmJsTest/`
- `rstudio-mobile/settings.gradle.kts`, root/web build files, and
  `scripts/web_toolchain_check.sh` only for the minimal pinned production
  Binaryen/repository and gate change.

**Out of scope**:

- Replacing WebR, introducing a web framework, or redesigning Android UI.
- Network package-policy changes.
- Visual imitation of desktop RStudio.

## Git workflow

- Branch `advisor/005-web-workbench-release`, isolated worktree.
- Conventional commit: `fix(web): restore panels and production build`.
- Do not push or modify the user's branch.

## Steps

### Step 1: Implement real tab/panel semantics

Use the DOM `hidden` property or add/remove the attribute; never assign string
booleans. Add an explicit `.panel[hidden] { display:none }` safeguard. Use a
tablist with tab roles, stable IDs, `aria-controls`, `aria-selected`, roving
`tabindex`, and Left/Right/Home/End keyboard behavior. Exactly one panel must be
visible/selected.

**Verify**: browser/Wasm tests click every tab and assert one visible panel;
keyboard tests assert focus and selection movement.

### Step 2: Meet phone accessibility basics

Raise interactive targets to at least 44 CSS px, use at least 16px form text,
add programmatic labels for every visible input, retain visible focus, and gate
hover-only styles with hover/pointer media queries. Do not disable pinch zoom.
Keep the 390px layout free of horizontal document overflow.

**Verify**: a DOM test asserts labels/roles; a headless browser check measures
targets and confirms `documentElement.scrollWidth <= clientWidth` at 390px.

### Step 3: Serialize state without keystroke-wide rewrites

Debounce persistence and flush on document switches/save/pagehide. Keep the
active editor model synchronous; a crash may lose only the bounded debounce
window, not other documents. Catch storage-quota errors and surface a status
without breaking editing.

**Verify**: fake-clock/unit or browser tests prove multiple rapid input events
cause one persistence write and quota failure leaves the editor usable.

### Step 4: Make WebR initialization single-flight

Replace the boolean-only guard with one shared initialization promise/deferred.
Concurrent environment and package refreshes must await the same init and an
init failure must be consistently reportable/retryable.

**Verify**: a fake backend/interoperability seam test issues two concurrent
calls and observes one init.

### Step 5: Fix and enforce production bundling

Configure the Kotlin/Binaryen repository/tool in `settings.gradle.kts` or the
supported Kotlin DSL so repository preference does not discard it. Pin the
version; do not add an unauthenticated arbitrary repository. Change the web
toolchain check to build production (development may remain an additional
check), and record a sensible JS/Wasm size budget.

**Verify**: production webpack succeeds from a clean dependency resolution and
the web gate exits 0.

## Done criteria

- [ ] Exactly one semantic panel is visible after mouse and keyboard actions.
- [ ] All visible phone controls are >=44px, fields are labeled and >=16px,
  focus is visible, and 390px has no horizontal page overflow.
- [ ] Document persistence is debounced and quota-safe.
- [ ] Concurrent startup performs one WebR initialization.
- [ ] Production webpack and Web/Wasm/shared tests pass.
- [ ] Only in-scope files change.

## STOP conditions

Stop if Binaryen can only be fetched from an unpinned/untrusted source, Kotlin
2.1.21 cannot support production bundling on the current Gradle line, tests
require a wholesale framework migration, or verification fails twice.

## Maintenance notes

Keep `[hidden]` authoritative regardless of component display rules. Any new
panel/input must join the tab state machine and label/target regression suite.
The release gate should compile production artifacts, not infer release health
from a development webpack build.

