import { expect, test } from "@playwright/test"

test("browser Wasm runif matches GNU R defaults, recycling, and RNG consumption", async ({
  page,
}) => {
  await page.goto("/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/index.ts")
    const runtime = new RRuntime({ timeoutMs: 30_000 })
    try {
      const seeded = await runtime.run("set.seed(1); runif(3)", "console")
      const recycled = await runtime.run(
        "set.seed(1); runif(2, c(10, 20), 30)",
        "console"
      )
      const equal = await runtime.run(
        "set.seed(1); runif(1, 4, 4); runif(1)",
        "console"
      )
      return {
        seeded: seeded.output,
        recycled: recycled.output,
        equal: equal.output,
      }
    } finally {
      runtime.dispose()
    }
  })
  expect(result.seeded).toContain("0.2655087 0.3721239 0.5728534")
  expect(result.recycled).toContain("15.31017 23.72124")
  expect(result.equal).toContain("4")
  expect(result.equal).toContain("0.2655087")
})

test("browser Wasm bounds captured console output and keeps the session usable", async ({
  page,
}) => {
  await page.goto("/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/index.ts")
    const runtime = new RRuntime({ timeoutMs: 30_000 })
    try {
      const large = await runtime.run(
        "cat(paste(rep('x', 2 * 1024 * 1024), collapse = ''))",
        "console"
      )
      const next = await runtime.run("1 + 1", "console")
      return { large: large.output, next: next.output }
    } finally {
      runtime.dispose()
    }
  })
  expect(result.large).toContain(
    "[captured console output truncated by runtime limit]"
  )
  expect(result.large.length).toBeLessThan(1024 * 1024 + 128)
  expect(result.next).toBe("[1] 2")
})
