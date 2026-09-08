import { cp, mkdir, readFile } from "node:fs/promises"
import { existsSync } from "node:fs"
import { assertBoundedMemory } from "./wasm-memory-limit.mjs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..")
const destination = join(root, "website/src/runtime/assets")
const configured = process.env.RPORT_WASM_PKG
const candidates = [configured && resolve(configured)].filter(Boolean)
const required = ["r_wasm.js", "r_wasm_bg.wasm"]
const existing = required.every((file) => existsSync(join(destination, file)))
if (!configured && existing) {
  assertBoundedMemory(await readFile(join(destination, "r_wasm_bg.wasm")))
  console.log(`Using prepared R Wasm runtime in ${destination}`)
  process.exit(0)
}
if (
  configured &&
  !required.every((file) => existsSync(join(resolve(configured), file)))
)
  throw new Error(
    `RPORT_WASM_PKG=${configured} is missing ${required.join(" or ")}.`
  )
if (!configured)
  candidates.push(
    join(
      root,
      "apps/workbench/webApp/build/dist/wasmJs/productionExecutable/rust-runtime"
    ),
    join(root, "crates/r-wasm/pkg")
  )
const source = candidates.find((candidate) =>
  required.every((file) => existsSync(join(candidate, file)))
)
if (!source)
  throw new Error(
    `R Wasm package missing. Set RPORT_WASM_PKG to a directory containing ${required.join(" and ")}, or build it with scripts/build_wasm_runtime.sh.`
  )
assertBoundedMemory(await readFile(join(source, "r_wasm_bg.wasm")))
await mkdir(destination, { recursive: true })
for (const file of [
  "r_wasm.js",
  "r_wasm_bg.wasm",
  "r_wasm.d.ts",
  "r_wasm_bg.wasm.d.ts",
  "package.json",
]) {
  if (existsSync(join(source, file)))
    await cp(join(source, file), join(destination, file))
}
const packageJson = join(destination, "package.json")
if (existsSync(packageJson)) {
  const metadata = JSON.parse(await readFile(packageJson, "utf8"))
  if (metadata.type !== "module")
    console.warn(
      "Copied Wasm package does not declare type=module; Vite may need an ESM package."
    )
}
console.log(`Prepared R Wasm runtime from ${source}`)
