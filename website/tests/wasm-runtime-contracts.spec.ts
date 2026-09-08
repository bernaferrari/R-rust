import { test, expect } from "@playwright/test"

test("interactive plots retain layers across commands in the actual Wasm worker", async ({ page }) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const split = new RRuntime()
    const combined = new RRuntime()
    try {
      await split.run("plot(1:2)", "interactive")
      const text = await split.run("2 + 2", "interactive")
      const actual = await split.run("lines(2:1, col='red')", "interactive")
      const expected = await combined.run("plot(1:2); lines(2:1, col='red')", "interactive")
      return {
        text: text.output,
        textHasPlot: !!text.png,
        actual: Array.from(actual.png ?? []),
        expected: Array.from(expected.png ?? []),
      }
    } finally {
      split.dispose()
      combined.dispose()
    }
  })
  expect(result.text).toContain("4")
  expect(result.textHasPlot).toBe(false)
  expect(result.actual.length).toBeGreaterThan(100)
  expect(result.actual).toEqual(result.expected)
})

test("named S4 signatures dispatch and reject invalid argument names recoverably", async ({ page }) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      await runtime.run("setGeneric('mix',function(x,y) standardGeneric('mix'))", "console")
      let error = ""
      try {
        await runtime.run("setMethod('mix',c(z='numeric'),function(x,y) 'wrong')", "console")
      } catch (cause) {
        error = String(cause)
      }
      const value = await runtime.run("setMethod('mix',c(y='character',x='numeric'),function(x,y) 'matched'); mix(1,'a')", "console")
      return { error, output: value.output }
    } finally {
      runtime.dispose()
    }
  })
  expect(result.error).toContain("signature argument")
  expect(result.output).toContain("matched")
})


test("S4 generic method tables are independent", async ({ page }) => {
  await page.goto("/console/")
  const output = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      return (await runtime.run("setGeneric('aa',function(x) standardGeneric('aa')); setMethod('aa','numeric',function(x) 'a'); setGeneric('bb',function(x) standardGeneric('bb')); setMethod('bb','numeric',function(x) 'b'); cat(aa(1),bb(1))", "console")).output
    } finally {
      runtime.dispose()
    }
  })
  expect(output).toBe("a b")
})

test("nested grob edits preserve the original in Wasm", async ({ page }) => {
  await page.goto("/console/")
  const output = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      return (await runtime.run("library(grid); g <- grobTree(grobTree(rectGrob(name='leaf',gp=gpar(fill='red',col='black')),name='inner')); before <- serialize(g,NULL); h <- editGrob(g,'leaf',gp=gpar(col='blue')); identical(before,serialize(g,NULL)) && identical(getGrob(h,gPath('inner','leaf'))$gp$fill,'red') && identical(getGrob(h,'leaf')$gp$col,'blue')", "console")).output
    } finally {
      runtime.dispose()
    }
  })
  expect(output).toContain("TRUE")
})
