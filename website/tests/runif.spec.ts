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
        `for (i in 1:2048) cat('${"x".repeat(1024)}')`,
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

test("browser memory and result budgets reject large requests and recover", async ({
  page,
}) => {
  await page.goto("/")
  const result = await page.evaluate(async () => {
    // @ts-expect-error Vite serves this module to browser tests.
    const { RRuntime } = await import("/src/runtime/index.ts")
    const runtime = new RRuntime({ timeoutMs: 20000 })
    const errors: string[] = []
    try {
      for (const code of ["numeric(100000000)", "rep('abcdef', 100000)"]) {
        try {
          await runtime.run(code, "console")
          errors.push("unexpected success")
        } catch (error) {
          errors.push(String(error))
        }
      }
      const recovered = await runtime.run("1 + 1", "console")
      // Verify the compiled module has a hard linear-memory maximum. Asking
      // beyond it must reject without allocating those pages.
      // @ts-expect-error Vite serves generated bindings.
      const { default: init } = await import("/src/runtime/assets/r_wasm.js")
      const module = await init()
      let bounded = false
      try {
        module.memory.grow(4097 - module.memory.buffer.byteLength / 65536)
      } catch (error) {
        bounded = error instanceof RangeError
      }
      return { errors, recovered: recovered.output, bounded }
    } finally {
      runtime.dispose()
    }
  })
  expect(result.errors[0]).toMatch(/alloc|budget|memory/i)
  expect(result.errors[1]).toContain("export budget")
  expect(result.recovered).toBe("[1] 2")
  expect(result.bounded).toBe(true)
})
