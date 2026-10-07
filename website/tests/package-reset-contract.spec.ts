import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

test("original package and compiled method contracts survive a public runtime reset", async ({
  page,
}) => {
  const names = [
    "namespace-s3-startup-contract",
    "base-print-methods-public-contract",
    "compiled-dots-public-contract",
    "vector-print-limits-public-contract",
    "print-digits-public-contract",
    "prmatrix-public-contract",
    "condition-handler-stack-public-contract",
  ]
  const contracts = names.map((name) => ({
    name,
    source: readFileSync(
      new URL(`../../crates/r-embed/tests/fixtures/${name}.R`, import.meta.url),
      "utf8"
    ),
    expected: readFileSync(
      new URL(
        `../../crates/r-embed/tests/fixtures/${name}.out`,
        import.meta.url
      ),
      "utf8"
    ),
  }))
  await page.goto("/console/")
  const output = await page.evaluate(async (contracts) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    const values = []
    try {
      for (let generation = 0; generation < 2; generation++) {
        if (generation) runtime.reset()
        for (const source of [
          "!exists('reset_fixture_marker', envir=.GlobalEnv, inherits=FALSE)",
          ...contracts.map(({ source }) => source),
          "reset_fixture_marker <- 42L; identical(reset_fixture_marker,42L)",
        ]) {
          const result = await runtime.run(source, "console")
          values.push({ output: result.output, error: result.error ?? null })
        }
      }
      return values
    } finally {
      runtime.dispose()
    }
  }, contracts)
  expect(output).toEqual(
    [0, 1].flatMap(() =>
      [
        "[1] TRUE\n",
        ...contracts.map(({ expected }) => expected),
        "[1] TRUE\n",
      ].map((output) => ({ output, error: null }))
    )
  )
})
