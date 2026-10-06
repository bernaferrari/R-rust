import { test, expect } from "@playwright/test"
import fixture from "./fixtures/wasm-stats-oracle.json" with { type: "json" }

test("portable FFT and all formerly gated RNGs match the pinned GNU R oracle", async ({
  page,
}) => {
  test.setTimeout(90000)
  await page.goto("/console/")
  for (const item of fixture.cases) {
    const started = Date.now()
    const result = await test.step(item.name, () =>
      page.evaluate(async (code) => {
        // Exercise the shipped worker and actual Wasm module, not a host-side substitute.
        const { RRuntime } = await import("/src/runtime/r-runtime.ts")
        const runtime = new RRuntime()
        try {
          return await runtime.run(code, "console")
        } finally {
          runtime.dispose()
        }
      }, item.code)
    )
    console.info(
      JSON.stringify({
        phase: "fft-rng-case",
        name: item.name,
        elapsedMs: Date.now() - started,
        workerDurationMs: result.durationMs,
      })
    )
    const output = result.output
    const actual = output.trim().split(",").map(Number)
    expect(actual.length, `${item.name}: ${output}`).toBe(item.expected.length)
    actual.forEach((value, index) => {
      const expected = item.expected[index]
      expect(
        Number.isFinite(value),
        `${item.name}[${index}]: ${output}`
      ).toBeTruthy()
      expect(
        Math.abs(value - expected),
        `${item.name}[${index}]: ${value} vs ${expected}`
      ).toBeLessThanOrEqual(2e-6 * Math.max(1, Math.abs(expected)))
    })
  }
})

test("FFT errors are recoverable and text-only calculations need no plot mode", async ({
  page,
}) => {
  await page.goto("/console/")
  const command = page.getByRole("textbox", { name: "R command", exact: true })
  await command.fill("mvfft(1:4)")
  await command.press("Enter")
  await expect(page.getByRole("log")).toContainText("series required")
  await command.fill("fft(c(1, 2, 3, 4))")
  await command.press("Enter")
  await expect(page.getByRole("log")).toContainText(/10\+0(?:\.0+)?(?:e\+00)?i/)
})
