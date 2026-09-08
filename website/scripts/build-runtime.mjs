import { spawnSync } from "node:child_process"
import { fileURLToPath } from "node:url"
import { resolve } from "node:path"
const root = fileURLToPath(new URL("../../", import.meta.url))
const output = resolve(root, "target/website-runtime")
const result = spawnSync(
  "bash",
  [
    resolve(root, "scripts/build_wasm_runtime.sh"),
    "--target",
    "web",
    "--out-dir",
    output,
  ],
  {
    cwd: root,
    stdio: "inherit",
    env: process.env,
  }
)
if (result.error) throw result.error
if (result.status !== 0) process.exit(result.status ?? 1)
process.env.RPORT_WASM_PKG = output
await import("./prepare-runtime.mjs")
