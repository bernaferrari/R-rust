import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

const code = readFileSync(
  new URL(
    "../../crates/r-embed/tests/fixtures/quote-options-contract.R",
    import.meta.url
  ),
  "utf8"
)
const expected = readFileSync(
  new URL(
    "../../crates/r-embed/tests/fixtures/quote-options-contract.out",
    import.meta.url
  ),
  "utf8"
)

test("GNU quote defaults and option restoration survive independent workers", async ({
  page,
}) => {
  await page.goto("/console/")
  const results = await page.evaluate(async (source) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const results = []
    for (let generation = 0; generation < 2; generation++) {
      const runtime = new RRuntime()
      try {
        results.push(await runtime.run(source, "console"))
        results.push(await runtime.run("1L + 1L", "console"))
      } finally {
        runtime.dispose()
      }
    }
    return results.map(({ output, error }) => ({ output, error }))
  }, code)
  expect(results).toEqual(
    [0, 1].flatMap(() => [
      { output: expected, error: undefined },
      { output: "[1] 2\n", error: undefined },
    ])
  )
})
