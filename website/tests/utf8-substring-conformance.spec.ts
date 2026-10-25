import { test, expect } from "@playwright/test"
import { readFileSync, readdirSync } from "node:fs"

const casesDirectory = new URL(
  "../../tests/conformance/cases/",
  import.meta.url
)
const contracts = readdirSync(casesDirectory)
  .filter((name) => /^\d+_utf8_substring_.*\.R$/.test(name))
  .sort()
  .map((name) => ({
    name,
    code: readFileSync(new URL(name, casesDirectory), "utf8"),
    expected: readFileSync(
      new URL(
        `../../tests/conformance/golden/${name.replace(/\.R$/, ".out")}`,
        import.meta.url
      ),
      "utf8"
    ),
  }))

test("GNU UTF-8 substring conformance survives independent browser workers", async ({
  page,
}) => {
  expect(contracts.length).toBeGreaterThanOrEqual(30)
  await page.goto("/console/")
  const results = await page.evaluate(async (cases) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const results = []
    for (let generation = 0; generation < 2; generation++) {
      const runtime = new RRuntime()
      try {
        for (const { name, code } of cases) {
          const result = await runtime.run(code, "console")
          results.push({ name, output: result.output, error: result.error })
        }
        const recovery = await runtime.run(
          'stopifnot(identical(substr("éaβ",2,2),"a"));1L+1L',
          "console"
        )
        results.push({
          name: "recovery",
          output: recovery.output,
          error: recovery.error,
        })
      } finally {
        runtime.dispose()
      }
    }
    return results
  }, contracts)
  expect(results).toEqual(
    [0, 1].flatMap(() => [
      ...contracts.map(({ name, expected }) => ({
        name,
        output: expected,
        error: undefined,
      })),
      { name: "recovery", output: "[1] 2\n", error: undefined },
    ])
  )
})
