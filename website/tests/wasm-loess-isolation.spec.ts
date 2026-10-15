import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

const contract = readFileSync(
  new URL(
    "../../crates/r-embed/tests/fixtures/loess-isolation.R",
    import.meta.url
  ),
  "utf8"
)
const gnuSurface = Array.from(
  readFileSync(
    new URL(
      "../../crates/r-embed/tests/fixtures/gnu-loess-surface.rds",
      import.meta.url
    )
  )
)

test("LOESS models survive later fits and serialization in a browser worker", async ({
  page,
}) => {
  await page.goto("/console/")
  const output = await page.evaluate(async (code) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      return (await runtime.run(code, "console")).output.trim()
    } finally {
      runtime.dispose()
    }
  }, contract)
  expect(output).toBe("[1] TRUE")
})

test("a fresh browser worker predicts from an independent GNU LOESS surface", async ({
  page,
}) => {
  await page.goto("/console/")
  const output = await page.evaluate(async (bytes) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      await runtime.importFile("gnu.rds", Uint8Array.from(bytes))
      return (
        await runtime.run(
          "saved<-readRDS('gnu.rds');max(abs(saved$expected-predict(saved$model,saved$q)))<1e-9",
          "console"
        )
      ).output.trim()
    } finally {
      runtime.dispose()
    }
  }, gnuSurface)
  expect(output).toBe("[1] TRUE")
})
