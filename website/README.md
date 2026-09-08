# Rove website

The site is a Vite React app with the R runtime running in a Web Worker through WebAssembly. Gallery previews are generated from the same runtime used by the playground.

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

`pnpm build` prepares the runtime, builds the client, creates an SSR bundle, and prerenders the landing page so the same `App` markup is available before hydration. The code editor uses a stable server fallback before loading its interactive client bundle.

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
