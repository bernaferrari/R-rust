import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

test("GNU branches and edited grid geometry work in the Wasm runtime", async ({
  page,
}) => {
  const bytes = readFileSync(
    new URL(
      "../../crates/r-embed/tests/fixtures/gnu-bytecode-branches/branch.rds",
      import.meta.url
    )
  )
  await page.goto("/console/")
  const result = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const branch = await runtime.run(
        `f <- unserialize(as.raw(c(${values}))); g <- unserialize(serialize(f,NULL)); cat(g(TRUE),g(FALSE))`,
        "console"
      )
      const grid = await runtime.run(
        "library(grid); grid.newpage(); g <- rectGrob(x=.2,width=.2,name='r'); h <- editGrob(g,x=unit(.75,'npc')); grid.draw(h); cat(identical(g$data$x,unit(.2,'npc')),length(h$data$x))",
        "interactive"
      )
      const ranks = await runtime.run(
        "cor(c(1,1,3),c(2,4,4),method='spearman')",
        "console"
      )
      const jit = await runtime.run(
        "old <- compiler::enableJIT(0); a <- compiler::enableJIT(2); b <- compiler::enableJIT(-1); restored <- compiler::enableJIT(old); cat(a,b)",
        "console"
      )
      return {
        branch: branch.output,
        grid: grid.output,
        image: !!grid.png,
        ranks: ranks.output,
        jit: jit.output,
      }
    } finally {
      runtime.dispose()
    }
  }, Array.from(bytes).join(","))
  expect(result.branch.trim()).toBe("1 2")
  expect(result.grid.trim()).toBe("TRUE 1")
  expect(result.image).toBe(true)
  expect(result.ranks.trim()).toBe("[1] 0.5")
  expect(result.jit.trim()).toBe("0 2")
})

test("interactive plots retain layers across commands in the actual Wasm worker", async ({
  page,
}) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const split = new RRuntime()
    const combined = new RRuntime()
    try {
      await split.run("plot(1:2)", "interactive")
      const text = await split.run("2 + 2", "interactive")
      const actual = await split.run("lines(2:1, col='red')", "interactive")
      const expected = await combined.run(
        "plot(1:2); lines(2:1, col='red')",
        "interactive"
      )
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

test("named S4 signatures dispatch and reject invalid argument names recoverably", async ({
  page,
}) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      await runtime.run(
        "setGeneric('mix',function(x,y) standardGeneric('mix'))",
        "console"
      )
      let error = ""
      try {
        await runtime.run(
          "setMethod('mix',c(z='numeric'),function(x,y) 'wrong')",
          "console"
        )
      } catch (cause) {
        error = String(cause)
      }
      const value = await runtime.run(
        "setMethod('mix',c(y='character',x='numeric'),function(x,y) 'matched'); mix(1,'a')",
        "console"
      )
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
      return (
        await runtime.run(
          "setGeneric('aa',function(x) standardGeneric('aa')); setMethod('aa','numeric',function(x) 'a'); setGeneric('bb',function(x) standardGeneric('bb')); setMethod('bb','numeric',function(x) 'b'); cat(aa(1),bb(1))",
          "console"
        )
      ).output
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
      return (
        await runtime.run(
          "library(grid); g <- grobTree(grobTree(rectGrob(name='leaf',gp=gpar(fill='red',col='black')),name='inner')); before <- serialize(g,NULL); h <- editGrob(g,'leaf',gp=gpar(col='blue')); identical(before,serialize(g,NULL)) && identical(getGrob(h,gPath('inner','leaf'))$gp$fill,'red') && identical(getGrob(h,'leaf')$gp$col,'blue')",
          "console"
        )
      ).output
    } finally {
      runtime.dispose()
    }
  })
  expect(output).toContain("TRUE")
})

test("GNU compiled closures and ANY signatures work in the browser runtime", async ({
  page,
}) => {
  const fixture = readFileSync(
    new URL(
      "../../crates/r-embed/tests/fixtures/gnu-compiled-captured.rds",
      import.meta.url
    )
  )
  const code = `g <- unserialize(as.raw(c(${Array.from(fixture).join(",")}))); cat(g(),g(4))`
  await page.goto("/console/")
  const result = await page.evaluate(async (code) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const compiled = await runtime.run(code, "console")
      const methods = await runtime.run(
        "setGeneric('wild',function(x,y) standardGeneric('wild')); setMethod('wild',c('ANY','ANY'),function(x,y) 'fallback'); setMethod('wild','numeric',function(x,y) 'number'); cat(wild(2,NULL),wild(NULL,TRUE))",
        "console"
      )
      return { compiled: compiled.output, methods: methods.output }
    } finally {
      runtime.dispose()
    }
  }, code)
  expect(result.compiled).toBe("9 22")
  expect(result.methods).toBe("number fallback")
})

test("long vector density and deparsing remain within string buffer allocation", async ({
  page,
}) => {
  await page.goto("/console/")
  const output = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      return (
        await runtime.run(
          'x <- seq(-4, 4, length.out=300); y <- dnorm(x); cat(length(y), "\\n"); cat(nchar(paste(deparse(x), collapse="")) > 1000)',
          "console"
        )
      ).output
    } finally {
      runtime.dispose()
    }
  })
  expect(output).toContain("300")
  expect(output).toContain("TRUE")
})

test("simulation drawing is silent while explicit NULL and custom method results remain visible", async ({
  page,
}) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const { examples } = await import("/src/data/examples.ts")
    const runtime = new RRuntime()
    try {
      const example = examples.find((item) => item.id === "constellation")!
      const drawing = await runtime.run(example.code, "interactive")
      const explicit = await runtime.run("print(NULL)", "interactive")
      const custom = await runtime.run(
        "x <- structure(1,class='probe'); plot.probe <- function(x,...) 42; plot(x)",
        "interactive"
      )
      return {
        drawing: drawing.output,
        image: !!drawing.png,
        explicit: explicit.output,
        custom: custom.output,
      }
    } finally {
      runtime.dispose()
    }
  })
  expect(result.drawing).toBe("")
  expect(result.image).toBe(true)
  expect(result.explicit.trim()).toBe("NULL")
  expect(result.custom).toContain("42")
})

test("GNU literal-return bytecode keeps exact values through browser round trips", async ({
  page,
}) => {
  const cases = [
    ["null", "NULL"],
    ["true", "TRUE"],
    ["false", "FALSE"],
  ]
  const programs = cases.map(([name, value]) => {
    const bytes = readFileSync(
      new URL(
        `../../crates/r-embed/tests/fixtures/gnu-${name}-closure.rds`,
        import.meta.url
      )
    )
    return `f <- unserialize(as.raw(c(${Array.from(bytes).join(",")}))); g <- unserialize(serialize(f,NULL)); cat(identical(f(),${value}),identical(g(),${value}),withVisible(g())$visible)`
  })
  await page.goto("/console/")
  const output = await page.evaluate(async (programs) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const outputs = []
      for (const code of programs)
        outputs.push((await runtime.run(code, "console")).output)
      return outputs
    } finally {
      runtime.dispose()
    }
  }, programs)
  expect(output).toEqual(["TRUE TRUE TRUE", "TRUE TRUE TRUE", "TRUE TRUE TRUE"])
})

test("GNU bytecode instructions take precedence over retained source in Wasm", async ({
  page,
}) => {
  const bytes = readFileSync(
    new URL(
      "../../crates/r-embed/tests/fixtures/gnu-true-closure.rds",
      import.meta.url
    )
  )
  const offset = bytes.indexOf(
    Buffer.from([0, 0, 0, 12, 0, 0, 0, 18, 0, 0, 0, 1])
  )
  expect(offset).toBeGreaterThanOrEqual(0)
  bytes[offset + 7] = 19
  await page.goto("/console/")
  const output = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      return (
        await runtime.run(
          `f <- unserialize(as.raw(c(${values}))); f()`,
          "console"
        )
      ).output
    } finally {
      runtime.dispose()
    }
  }, Array.from(bytes).join(","))
  expect(output.trim()).toBe("[1] FALSE")
})

test("interpreted active bindings and caller-scoped grid methods work in Wasm", async ({ page }) => {
  await page.goto("/console/")
  const output = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const result = await runtime.run(
        `old <- compiler::enableJIT(0)
         e <- new.env()
         makeActiveBinding('x', function() { gc(); list(b=2) }, e)
         f <- function() c(list(a=1),x)
         environment(f) <- e
         active <- identical(f(),list(a=1,b=2))
         library(grid)
         editDetails.custom <- function(x,specs) x$foo + specs$foo
         g <- structure(list(foo=1,name='g',gp=gpar(),vp=NULL),class=c('custom','grob','gDesc'))
         edited <- editGrob(g,foo=3)
         alias <- identity
         called <- do.call(alias,list(quote(x)),quote=TRUE,envir=e)
         cat(active,edited,identical(called,quote(x)))`,
        "console"
      )
      return result.output
    } finally {
      runtime.dispose()
    }
  })
  expect(output.trim()).toBe("TRUE 6 TRUE")
})
