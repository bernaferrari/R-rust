# Rove website

The site is a Vite React app with the R runtime running in a Web Worker through WebAssembly. The 16 gallery examples and their previews use the same runtime as the playground.

Examples run automatically after a 650 ms typing pause. Choose **Run manually** to make edits without executing them; **Run code** (or Cmd/Ctrl+Enter) then runs the current code. Stop terminates the worker and stays stopped until another edit or explicit run. Theme follows the system on first visit and remembers an explicit light/dark choice.

## Local setup

From this directory:

```bash
pnpm install --frozen-lockfile
RPORT_WASM_PKG=/absolute/path/to/r-wasm/pkg pnpm dev
```

`RPORT_WASM_PKG` must contain `r_wasm.js` and `r_wasm_bg.wasm`. An existing prepared runtime is reused when the variable is omitted. To build a fresh package from a clean checkout, run `pnpm build:runtime`; it invokes the repository-root build with `--target web` and an absolute output path at `target/website-runtime`, then prepares that exact package. An explicit `RPORT_WASM_PKG` pointing at a missing or incomplete package fails loudly.

The browser model in Local AI loads only after the user requests it and requires WebGPU. Ollama is optional; run it locally and allow this site's origin with `OLLAMA_ORIGINS` if the browser blocks localhost requests.

## Checks and production build

```bash
pnpm typecheck
pnpm lint
pnpm build
pnpm test:unit
pnpm exec playwright install chromium
pnpm test:e2e
```

`pnpm build` prepares the runtime, builds the client, creates an SSR bundle, and prerenders all seven pages so the same `App` markup is available before hydration. The code editor uses a stable server fallback before loading its interactive client bundle.

The gallery preview generator needs a running Vite server and Playwright:

```bash
SERVER_URL=http://127.0.0.1:5173 \
NODE_PATH=/absolute/path/to/playwright/node_modules \
node scripts/generate-previews.cjs
```

The generator runs every example in a fresh browser R worker, checks that plots contain PNG bytes or console examples contain output, and writes plot previews to `public/examples/`.

Prerequisites for `build:runtime`: Rust nightly-2026-08-25 with `rust-src` and
`wasm32-unknown-unknown`, plus `wasm-pack` on PATH. The root build script preserves
Wasm exception handling so R errors and control flow can unwind safely.

The website uses CPU Vello rendering in an isolated worker. The separate optional
GPU package exposes direct canvas and native window presentation; see
[`r-device-vello-gpu`](../crates/r-device-vello-gpu/README.md).
The AI model is downloaded only after **Load model**. Its weights are separate
from the R Wasm download. Browser model support depends on WebGPU and available
memory; Ollama provides another local inference path.

Production output is `dist/`; serve it over HTTPS with `application/wasm` for Wasm
files and gzip or Brotli enabled. No server-side compute or API key is required.
The browser AI smoke was independently exercised with genuine model weights in
Chrome, followed by execution and rendering of the generated R code. Ordinary
CI tests mock Ollama's HTTP response to keep that contract deterministic.

## Focused pages and search metadata

The footer links to `/console/`, `/editor/`, `/examples/`, `/local-ai/`, `/embedding/`, and
`/compatibility/`. Each is rendered to its own HTML file with a unique title,
description, heading and Open Graph metadata. The editor is the same worker-backed
playground without the marketing sections. Gallery cards open the selected recipe
in that editor. No account or shared-script service is involved.

Set the full public URL when building for deployment:

```bash
SITE_URL=https://your-domain.example/ pnpm build
```

Replace the example with the actual website URL. A subdirectory is supported:
`SITE_URL=https://your-domain.example/rove/ pnpm build` sets asset and page paths
accordingly. The build generates canonical URLs, Open Graph image/URL metadata,
`sitemap.xml`, and `robots.txt` from that address. Without `SITE_URL`, local builds
omit canonical URLs and the XML sitemap instead of publishing a guessed domain.

Serve `dist/` with directory index support (`/editor/` → `/editor/index.html`).
Use `404.html` as the host's custom **404 response**, rather than rewriting every
unknown URL to the homepage with status 200. If hosted below a subdirectory,
include the sitemap location in the domain's root robots.txt as appropriate.
The additional pages are useful destinations, not a promise of search ranking.

## Runtime resource contract

The shipped worker uses a **64 MiB R arena budget**, a **500,000-node budget**,
a **1 MiB result export admission budget**, and a **256 MiB hard maximum on Wasm
linear memory**. Captured stdout/stderr share a 1 MiB cap. The result admission
budget conservatively counts values, repeated references, strings and metadata
before formatting or host copying; it is not a promise to export every object
whose eventual text fits in 1 MiB. Excessive nesting is rejected as well.

Ordinary budget errors are reported without replacing the value with a successful
NULL. A fatal Wasm trap closes and resets the worker. The next request creates a
fresh session. Browser/JavaScript overhead, GPU resources and local AI weights
are outside the Wasm memory ceiling. Native embedding defaults remain unlimited;
hosts can set arena and result limits and use OS process controls when they need
a process-wide memory boundary.

`prepare-runtime.mjs` validates the memory declaration in both newly supplied and
already prepared Wasm artifacts, so an older unlimited package cannot silently
replace the bounded runtime. Rebuild old packages with `pnpm build:runtime`.

### Interactive R console

`/console/` is a chat-style REPL using Shadcn Message, Bubble, and Message Scroller. One isolated Wasm worker keeps variables between commands until navigation, reset, or a fatal runtime error. Enter runs; Shift+Enter inserts a line. Text and plots are returned automatically from a single evaluation. Reuse a command or download its plot. Each plot command opens a fresh device; put plot overlays in the same command. Stop resets the worker and its variables. The latest 500 commands are retained in browser memory and displayed with TanStack Virtual, with plot URLs released when discarded. Three starter conversations run real commands and leave a suggested follow-up with their variables available.

### Portable statistics checks

The browser dispatches `fft`, `mvfft`, and the statistical random generators to
the same portable Rust handlers used by native sessions. The former Wasm-only
unavailable stubs have been removed. `tests/wasm-stats.spec.ts` executes the real
worker against 29 pinned-GNU-R numerical fixtures, including forward/inverse FFT,
array and column transforms, 17 distribution samplers, parameter recycling and
RNG stream continuation. The tolerance accounts for R's printed numeric precision;
this is numerical fixture coverage, not universal floating-point parity.

Regenerate the fixture using the pinned oracle (the script checks its revision):

```sh
python3 website/scripts/generate-stats-oracle.py /path/to/pinned/bin/Rscript
```
