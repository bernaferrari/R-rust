import { test, expect } from "@playwright/test"

test("session files work with standard R reads, source and writes", async ({
  page,
}) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const session = new RRuntime()
    const independent = new RRuntime()
    try {
      await session.run('writeLines("fresh", "fresh.txt")', "console")
      const fresh = new TextDecoder().decode(
        await session.exportFile("fresh.txt")
      )
      if (fresh !== "fresh\n")
        throw new Error("fresh session file creation failed")
      await session.importFile(
        "data.csv",
        new TextEncoder().encode("value\n10\n20\n30\n")
      )
      await session.importFile(
        "analysis.R",
        new TextEncoder().encode("answer <- 42")
      )
      const beforeSource = await session.run('exists("answer")', "console")
      const mean = await session.run(
        'data <- read.csv("data.csv"); mean(data$value)',
        "console"
      )
      await session.run('source("analysis.R")', "console")
      const afterSource = await session.run("answer", "console")
      await session.run(
        'writeLines(c("first", "second"), "result.txt")',
        "console"
      )
      const exported = new TextDecoder().decode(
        await session.exportFile("result.txt")
      )
      const otherFiles = await independent.listFiles()
      let invalid = false
      try {
        await session.importFile("../escape", new Uint8Array([1]))
      } catch {
        invalid = true
      }
      const stillReadable = new TextDecoder().decode(
        await session.exportFile("data.csv")
      )
      session.reset()
      return {
        before: beforeSource.output,
        mean: mean.output,
        after: afterSource.output,
        exported,
        otherFiles,
        invalid,
        stillReadable,
        afterReset: await session.listFiles(),
      }
    } finally {
      session.dispose()
      independent.dispose()
    }
  })
  expect(result.before).toContain("FALSE")
  expect(result.mean).toContain("20")
  expect(result.after).toContain("42")
  expect(result.exported).toBe("first\nsecond\n")
  expect(result.otherFiles).toEqual([])
  expect(result.invalid).toBe(true)
  expect(result.stillReadable).toBe("value\n10\n20\n30\n")
  expect(result.afterReset).toEqual([])
})

test("console imports a file without executing its code", async ({ page }) => {
  await page.goto("/console/")
  await page.getByText("Session files", { exact: true }).click()
  await page.getByLabel("Import a session file").setInputFiles({
    name: "hello.R",
    mimeType: "text/plain",
    buffer: Buffer.from("hidden_answer <- 7"),
  })
  await expect(page.locator(".r-chat-files-panel")).toContainText("hello.R")
  const input = page.getByRole("textbox", { name: "R command", exact: true })
  await input.fill('exists("hidden_answer")')
  await input.press("Enter")
  await expect(page.getByRole("log")).toContainText("FALSE")
  await input.fill('source("hello.R"); hidden_answer')
  await input.press("Enter")
  await expect(page.getByRole("log")).toContainText("[1] 7")
})
