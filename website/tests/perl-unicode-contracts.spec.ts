import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

const contracts = [
  "perl-class-family-contract",
  "perl-empty-pattern-contract",
  "gregexpr-capture-attribution-contract",
].map((name) => ({
  code: readFileSync(
    new URL(`../../crates/r-embed/tests/fixtures/${name}.R`, import.meta.url),
    "utf8"
  ),
  expected: readFileSync(
    new URL(`../../crates/r-embed/tests/fixtures/${name}.out`, import.meta.url),
    "utf8"
  ),
}))

test("UTF-8 Perl classes preserve replacements and capture offsets in isolated workers", async ({
  page,
}) => {
  await page.goto("/console/")
  const results = await page.evaluate(async (cases) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const values = []
    for (let generation = 0; generation < 2; generation++) {
      const runtime = new RRuntime()
      try {
        for (const { code } of cases) {
          const result = await runtime.run(code, "console")
          values.push({ output: result.output, error: result.error })
        }
        const recovery = await runtime.run("1L+1L", "console")
        values.push({ output: recovery.output, error: recovery.error })
      } finally {
        runtime.dispose()
      }
    }
    return values
  }, contracts)
  expect(results).toEqual(
    [0, 1].flatMap(() => [
      ...contracts.map(({ expected }) => ({
        output: expected,
        error: undefined,
      })),
      { output: "[1] 2\n", error: undefined },
    ])
  )
})
