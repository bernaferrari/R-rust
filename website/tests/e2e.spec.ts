import { expect, test } from "@playwright/test"
import AxeBuilder from "@axe-core/playwright"

test("runs real R code and reports an evaluation error", async ({ page }) => {
  await page.goto("/")
  await page
    .getByRole("combobox", { name: "Choose an R example" })
    .selectOption("matrix")
  await page.getByRole("button", { name: "Console" }).click()
  await page.getByRole("button", { name: "Run code" }).click()
  await expect(page.locator(".console-output")).toContainText("Solution:", {
    timeout: 30_000,
  })

  const editor = page.locator(".cm-content")
  await editor.click()
  await page.keyboard.press("ControlOrMeta+A")
  await page.keyboard.type("stop('intentional test error')")
  await page.getByRole("button", { name: "Run code" }).click()
  await expect(page.getByRole("alert")).toContainText(
    "intentional test error",
    { timeout: 30_000 }
  )
})

test("keeps seeded random numbers and samples repeatable across sessions", async ({
  page,
}) => {
  await page.goto("/")
  const values = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/index.ts")
    const run = async () => {
      const runtime = new RRuntime({ timeoutMs: 30_000 })
      try {
        return (
          await runtime.run(
            "set.seed(42); print(round(rnorm(4), 6)); print(sample(1:6, 8, replace = TRUE))",
            "console"
          )
        ).output
      } finally {
        runtime.dispose()
      }
    }
    return [await run(), await run()]
  })
  expect(values[0]).toBe(values[1])
  expect(values[0]).toContain("[1]")
})

test("does not download the Wasm runtime before the first run", async ({
  page,
}) => {
  const runtimeRequests: string[] = []
  page.on("request", (request) => {
    if (request.url().includes("r_wasm")) runtimeRequests.push(request.url())
  })
  await page.goto("/")
  await page.waitForLoadState("networkidle")
  expect(runtimeRequests).toEqual([])
  await page.getByRole("button", { name: "Run code" }).click()
  await expect.poll(() => runtimeRequests.length).toBeGreaterThan(0)
})

test("stops a running R worker and clears its session", async ({ page }) => {
  await page.goto("/")
  const editor = page.locator(".cm-content")
  await editor.click()
  await page.keyboard.press("ControlOrMeta+A")
  await page.keyboard.type("repeat {}")
  await page.getByRole("button", { name: "Run code" }).click()
  await expect(page.getByRole("button", { name: "Stop & reset" })).toBeVisible()
  await page.getByRole("button", { name: "Stop & reset" }).click()
  await expect(page.getByRole("status")).toContainText("Session cleared")
})

test("filters the gallery, handles an empty search, and selects a recipe", async ({
  page,
}) => {
  await page.goto("/")
  await page.getByRole("button", { name: "Statistics" }).click()
  await expect(page.locator(".example-card")).toHaveCount(3)
  const search = page.getByRole("searchbox", { name: "Search examples" })
  await search.fill("nothing matches this")
  await expect(page.getByText("No examples found.")).toBeVisible()
  await page.getByRole("button", { name: "Show all examples" }).click()
  await search.fill("sunflower")
  await page.getByRole("button", { name: "Try Nature has a formula" }).click()
  await expect(
    page.getByRole("combobox", { name: "Choose an R example" })
  ).toHaveValue("sunflower")
  await expect(page.locator(".cm-content")).toContainText("Phyllotaxis")
})

test("drafts through a mocked Ollama endpoint and hands code to the playground", async ({
  page,
}) => {
  await page.route("**/api/tags", async (route) =>
    route.fulfill({ json: { models: [{ name: "mock-r" }] } })
  )
  await page.route("**/api/chat", async (route) =>
    route.fulfill({
      json: { message: { content: "```r\nmean(c(4, 8, 15, 16))\n```" } },
    })
  )
  await page.goto("/")
  await page.getByRole("button", { name: "Ollama" }).click()
  await page.getByRole("button", { name: "Connect" }).click()
  await expect(page.getByRole("combobox", { name: "Model" })).toHaveValue(
    "mock-r"
  )
  await page.getByRole("button", { name: "Draft code" }).click()
  await expect(
    page.getByRole("textbox", { name: "Editable R code draft" })
  ).toHaveValue("mean(c(4, 8, 15, 16))")
  await page.getByRole("button", { name: "Use this code" }).click()
  await expect(page.locator(".cm-content")).toContainText(
    "mean(c(4, 8, 15, 16))"
  )
  await page.getByRole("button", { name: "Run code" }).click()
  await expect(page.locator(".console-output")).toContainText("10.75", {
    timeout: 30_000,
  })
})

test("stays usable on mobile and has no axe violations", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto("/")
  const overflow = await page.evaluate(
    () =>
      document.documentElement.scrollWidth <=
      document.documentElement.clientWidth
  )
  expect(overflow).toBe(true)
  const results = await new AxeBuilder({ page }).analyze()
  expect(results.violations).toEqual([])
})
