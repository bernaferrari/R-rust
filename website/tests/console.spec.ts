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
  await page.getByRole("combobox", { name: "Command output" }).click()
  await page.getByRole("option", { name: "Plot output", exact: true }).click()
  await run("plot(1:5)")
  await expect(
    page.getByRole("img", { name: "Plot from command 5" })
  ).toBeVisible()
  await page.getByRole("button", { name: "New session", exact: true }).click()
  await expect(page.getByRole("log")).not.toContainText("[1] 42")
  await page.getByRole("combobox", { name: "Command output" }).click()
  await page.getByRole("option", { name: "Text output", exact: true }).click()
  await run('exists("answer")')
  await expect(page.getByRole("log")).toContainText("FALSE")
})
test("console mobile layout and multiline input", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto("/console/")
  await page.getByRole("button", { name: "Start with a little data" }).click()
  const command = page.getByRole("textbox", { name: "R command", exact: true })
  await expect(command).toHaveValue(/temperatures/)
  await command.press("ControlOrMeta+End")
  await command.press("Shift+Enter")
  await expect(page.getByRole("log")).not.toContainText("You ·")
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth
    )
  ).toBeTruthy()
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
