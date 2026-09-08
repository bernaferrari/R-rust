import { test, expect } from "@playwright/test"
test("console preserves variables, recovers from errors, renders plots and resets", async ({
  page,
}) => {
  await page.goto("/console/")
  const command = page.getByRole("textbox", { name: "R command", exact: true })
  async function run(code: string) {
    await command.fill(code)
    await page.getByRole("button", { name: "Run command", exact: true }).click()
    await expect(
      page.getByRole("button", { name: "Run command", exact: true })
    ).toBeVisible({ timeout: 30000 })
  }
  await run("answer <- 21")
  await run("answer * 2")
  await expect(page.getByRole("log")).toContainText("[1] 42")
  await run('stop("try again")')
  await expect(page.getByRole("log")).toContainText("try again")
  await run("answer + 1")
  await expect(page.getByRole("log")).toContainText("[1] 22")
  await run("plot(1:5)")
  await expect(
    page.getByRole("img", { name: "Plot from command 5" })
  ).toBeVisible()
  await page.getByRole("button", { name: "New session", exact: true }).click()
  await expect(page.getByRole("log")).not.toContainText("[1] 42")
  await run('exists("answer")')
  await expect(page.getByRole("log")).toContainText("FALSE")
})
test("console mobile layout and multiline input", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto("/console/")
  await page.getByRole("button", { name: /A week in numbers/ }).click()
  await expect(page.getByRole("status").last()).toContainText("Your turn", {
    timeout: 30000,
  })
  const command = page.getByRole("textbox", { name: "R command", exact: true })
  await expect(command).toHaveValue(/temperatures/)
  await command.press("ControlOrMeta+End")
  await command.press("Shift+Enter")
  await expect(page.locator(".r-chat-turn")).toHaveCount(3)
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth
    )
  ).toBeTruthy()
})

test("interactive errors keep partial console output and plots", async ({
  page,
}) => {
  await page.goto("/console/")
  const command = page.getByRole("textbox", { name: "R command", exact: true })
  await command.fill("cat('partial\\n'); plot(1:2); stop('boom')")
  await page.getByRole("button", { name: "Run command", exact: true }).click()
  await expect(page.getByRole("log")).toContainText("partial")
  await expect(page.getByRole("log")).toContainText("boom")
  await expect(page.getByRole("img", { name: "Plot from command 1" })).toBeVisible()
})

test("stop resets a busy session and remains usable", async ({ page }) => {
  await page.goto("/console/")
  const command = page.getByRole("textbox", { name: "R command", exact: true })
  await command.fill("repeat {}")
  await page.getByRole("button", { name: "Run command", exact: true }).click()
  await page.getByRole("button", { name: "Stop & reset" }).click()
  await command.fill("2 + 3")
  await page.getByRole("button", { name: "Run command", exact: true }).click()
  await expect(page.getByRole("log")).toContainText("[1] 5", { timeout: 30000 })
})

for (const title of [
  "A week in numbers",
  "Find the rhythm",
  "Let chance speak",
]) {
  test(`example conversation can continue: ${title}`, async ({ page }) => {
    await page.goto("/console/")
    await page.getByRole("button", { name: new RegExp(title) }).click()
    await expect(page.getByRole("status").last()).toContainText("Your turn", {
      timeout: 30000,
    })
    await expect(page.getByRole("log").getByRole("img")).toHaveCount(1)
    const count = await page.locator(".r-chat-turn").count()
    await page.getByRole("button", { name: "Run command", exact: true }).click()
    await expect(
      page.getByRole("button", { name: "Run command", exact: true })
    ).toBeVisible({ timeout: 30000 })
    await expect(page.locator(".r-chat-turn")).toHaveCount(count + 1)
    await expect(
      page.locator('[data-slot="bubble"][data-variant="destructive"]')
    ).toHaveCount(0)
  })
}

test("virtual history caps at 500 and submitting returns to the bottom", async ({
  page,
}) => {
  test.setTimeout(120000)
  await page.addInitScript(() => {
    class FakeWorker {
      onmessage: ((event: { data: unknown }) => void) | null = null
      postMessage(request: { id: number; code: string }) {
        queueMicrotask(() =>
          this.onmessage?.({
            data: {
              id: request.id,
              ok: true,
              output: `result ${request.code}`,
              durationMs: 1,
            },
          })
        )
      }
      terminate() {}
    }
    Object.defineProperty(window, "Worker", { value: FakeWorker })
  })
  await page.goto("/console/")
  const command = page.getByRole("textbox", { name: "R command", exact: true })
  for (let i = 1; i <= 501; i++) {
    await command.fill(String(i))
    await command.press("Enter")
    await expect(
      page.getByRole("button", { name: "Run command", exact: true })
    ).toBeVisible()
  }
  await expect(page.getByLabel("History count")).toHaveText(
    "500 / 500 commands"
  )
  expect(await page.locator(".r-chat-turn").count()).toBeLessThan(30)
  const scroll = page.getByRole("region", { name: "R command history" })
  await scroll.evaluate((node) => {
    node.scrollTop = 0
  })
  await expect(
    page.getByRole("button", { name: "Reuse command 2", exact: true })
  ).toBeVisible()
  await expect(
    page.getByRole("button", { name: "Reuse command 1", exact: true })
  ).toHaveCount(0)
  await command.fill("502")
  await command.press("Enter")
  await expect(page.getByRole("log")).toContainText("result 502")
  await expect
    .poll(() =>
      scroll.evaluate(
        (node) => node.scrollHeight - node.scrollTop - node.clientHeight
      )
    )
    .toBeLessThan(100)
})
