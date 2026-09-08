// Generate the example gallery from the real browser Wasm runtime.
// Usage: NODE_PATH=... SERVER_URL=http://127.0.0.1:5173 node scripts/generate-previews.cjs
const fs = require("node:fs/promises")
const path = require("node:path")
const { chromium } = require("playwright")

const serverUrl = process.env.SERVER_URL || "http://127.0.0.1:5173"
const outputDir = path.resolve(__dirname, "../public/examples")

async function main() {
  await fs.mkdir(outputDir, { recursive: true })
  const browser = await chromium.launch({ headless: true })
  const page = await browser.newPage({
    viewport: { width: 900, height: 700 },
    deviceScaleFactor: 1,
  })
  const consoleErrors = []
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text())
  })
  page.on("pageerror", (error) => consoleErrors.push(error.message))
  try {
    await page.goto(serverUrl, { waitUntil: "networkidle" })
    const results = await page.evaluate(async () => {
      const [{ RRuntime }, { examples, getExampleCode }] = await Promise.all([
        import("/src/runtime/index.ts"),
        import("/src/data/examples.ts"),
      ])
      const outputs = []
      for (const example of examples) {
        const runtime = new RRuntime({ timeoutMs: 30_000 })
        try {
          const result = await runtime.run(getExampleCode(example, false), example.mode)
          const darkResult = await runtime.run(
            getExampleCode(example, true),
            example.mode
          )
          if (
            example.mode === "plot" &&
            (!result.png || result.png.length < 100 || !darkResult?.png || darkResult.png.length < 100)
          ) {
            throw new Error("plot returned no meaningful PNG")
          }
          if (example.mode === "console" && !result.output.trim()) {
            throw new Error("console example returned empty output")
          }
          if (example.mode === "console" && !darkResult.output.trim()) {
            throw new Error("dark console example returned empty output")
          }
          outputs.push({
            id: example.id,
            mode: example.mode,
            output: result.output,
            png: result.png ? Array.from(result.png) : null,
            darkPng: darkResult?.png ? Array.from(darkResult.png) : null,
            durationMs: result.durationMs,
          })
        } catch (error) {
          outputs.push({
            id: example.id,
            mode: example.mode,
            error: error instanceof Error ? error.message : String(error),
          })
        } finally {
          runtime.dispose()
        }
      }
      return outputs
    })
    const failures = []
    for (const result of results) {
      if (result.error) {
        failures.push(`${result.id}: ${result.error}`)
        continue
      }
      if (result.png)
        await fs.writeFile(
          path.join(outputDir, `${result.id}.png`),
          Buffer.from(result.png)
        )
      if (result.darkPng)
        await fs.writeFile(
          path.join(outputDir, `${result.id}-dark.png`),
          Buffer.from(result.darkPng)
        )
      process.stdout.write(
        `${result.id}: ok (${result.durationMs}ms)${result.output ? ` — ${result.output.trim().split("\n")[0]}` : ""}\n`
      )
    }
    if (consoleErrors.length)
      failures.push(`browser console: ${consoleErrors.join(" | ")}`)
    if (failures.length) {
      process.stderr.write(
        `\nPreview failures:\n${failures.map((failure) => `- ${failure}`).join("\n")}\n`
      )
      process.exitCode = 1
    } else
      process.stdout.write(
        `Generated ${results.filter((result) => result.png).length} PNG previews in ${outputDir}\n`
      )
  } finally {
    await browser.close()
  }
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
