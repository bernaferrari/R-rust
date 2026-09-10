import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

test("namespace bytecode, automatic file opening and sink routing match GNU", async ({ page }) => {
  const bytes = readFileSync(new URL("../../crates/r-embed/tests/fixtures/gnu-bytecode-checkfun/base-abs.rds", import.meta.url))
  await page.goto("/console/")
  const output = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const result = []
      for (const code of [
        `f<-unserialize(as.raw(c(${values})));identical(f(c(-2L,3L)),c(2L,3L))`,
        "con<-file('auto.txt');writeLines('hello',con);!isOpen(con)&&identical(readLines(con),'hello')&&!isOpen(con)",
        "local({a<-'outer.txt';b<-'inner.txt';sink(a);sink(b);cat('b');sink();cat('a');sink();identical(readLines(a,warn=FALSE),'a')&&identical(readLines(b,warn=FALSE),'b')})",
        "local({p<-'split.txt';v<-capture.output({sink(p,split=TRUE);cat('both');sink()});identical(v,'both')&&identical(readLines(p,warn=FALSE),'both')})",
      ]) result.push((await runtime.run(code,"console")).output.trim())
      return result
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(output).toEqual(Array(4).fill("[1] TRUE"))
})

test("GNU computed calls and deferred capture connections work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL("../../crates/r-embed/tests/fixtures/gnu-bytecode-checkfun/callable-argument.rds", import.meta.url))
  await page.goto("/console/")
  const output = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const result = []
      for (const code of [
        `f<-unserialize(as.raw(c(${values})));identical(f(abs,-4L),4L)`,
        "con<-file('deferred.txt');before<-isOpen(con);invisible(capture.output(cat('hello'),file=con));closed<-tryCatch({isOpen(con);FALSE},error=function(e)TRUE);!before&&closed&&identical(readLines('deferred.txt',warn=FALSE),'hello')",
      ]) result.push((await runtime.run(code,"console")).output.trim())
      return result
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(output).toEqual(Array(2).fill("[1] TRUE"))
})

test("GNU primitive calls and streaming file capture work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL("../../crates/r-embed/tests/fixtures/gnu-bytecode-builtin-calls/nested.rds", import.meta.url))
  await page.goto("/console/")
  const output = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const result = []
      for (const code of [
        `f<-unserialize(as.raw(c(${values})));identical(f(-4L),4L)`,
        "v<-withVisible(capture.output(cat('hello'),file='capture.txt'));!v$visible&&is.null(v$value)&&identical(readLines('capture.txt',warn=FALSE),'hello')",
        "invisible(tryCatch(capture.output({cat('partial');stop('boom')},file='partial.txt'),error=function(e)NULL));identical(readLines('partial.txt',warn=FALSE),'partial')",
      ]) result.push((await runtime.run(code,"console")).output.trim())
      return result
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(output).toEqual(Array(3).fill("[1] TRUE"))
})

test("literal GNU call arguments, QR transforms and selective capture work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL("../../crates/r-embed/tests/fixtures/gnu-bytecode-constant-args/pushconstarg.rds", import.meta.url))
  const stream = Buffer.alloc(40)
  ;[12,23,1,34,2,29,3,38,0,1].forEach((word,index) => stream.writeInt32BE(word,index*4))
  const offset = bytes.indexOf(stream)
  expect(offset).toBeGreaterThan(-1)
  bytes.writeInt32BE(1,offset+16)
  await page.goto("/console/")
  const results = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const output = []
      for (const code of [
        `target<-function(a,b)identical(a,1L);f<-unserialize(as.raw(c(${values})));identical(f('value'),FALSE)`,
        "q<-qr(matrix(1:6,3,2));max(abs(qr.qty(q,qr.qy(q,1:3))-1:3))<1e-10",
        "identical(capture.output(message('hello'),type='message'),'hello')",
        "identical(capture.output(capture.output(cat('tee\\n'),split=TRUE)),c('tee','[1] \"tee\"'))",
      ]) output.push((await runtime.run(code,"console")).output.trim())
      return output
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(results).toEqual(Array(4).fill("[1] TRUE"))
})

test("GNU superassignment, evalq, Q factors and output capture work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL("../../crates/r-embed/tests/fixtures/gnu-bytecode-assignment/setvar2.rds", import.meta.url))
  await page.goto("/console/")
  const results = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const output = []
      for (const code of [
        `f<-unserialize(as.raw(c(${values})));g<-unserialize(serialize(f,NULL));identical(g(41L),41L)&&identical(y,41L)`,
        "local({y<-3;identical(evalq(x+y,list(x=2)),5)&&!withVisible(evalq(invisible(1)))$visible})",
        "q<-qr(matrix(1:6,3,2));Q<-qr.Q(q,complete=TRUE);max(abs(crossprod(Q)-diag(3)))<1e-10&&max(abs(Q%*%qr.R(q,complete=TRUE)-matrix(1:6,3,2)))<1e-10",
        "identical(capture.output(1,invisible(2),3),c('[1] 1','[1] 3'))",
        "identical(capture.output({tryCatch(capture.output(stop('boom')),error=function(e)NULL);cat('after')}),'after')",
      ]) output.push((await runtime.run(code,"console")).output.trim())
      return output
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(results).toEqual(Array(5).fill("[1] TRUE"))
})

test("named GNU calls, real QR and method existence work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL("../../crates/r-embed/tests/fixtures/gnu-bytecode-named-calls/nested-reversed-tags.rds", import.meta.url))
  await page.goto("/console/")
  const results = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const output = []
      for (const code of [
        `target<-function(a,b)paste(a,b,sep='/');f<-unserialize(as.raw(c(${values})));g<-unserialize(serialize(f,NULL));identical(g('left','right'),'left/right')`,
        "q<-qr(cbind(c(1,2,3),c(2,4,6)));identical(q$rank,1L)&&identical(dim(qr.R(q)),c(2L,2L))",
        "q<-qr(matrix(1:6,3,2),LAPACK=TRUE);identical(q$pivot,c(2L,1L))&&identical(q$rank,2L)",
        "local({setClass('ExistA');setClass('ExistB',contains='ExistA');setGeneric('existProbe',function(x)standardGeneric('existProbe'));setMethod('existProbe','ExistA',function(x)1);existsMethod(as.name('existProbe'),'ExistA')&&!existsMethod('existProbe','ExistB')&&hasMethod('existProbe','ExistB')})",
      ]) output.push((await runtime.run(code,"console")).output.trim())
      return output
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(results).toEqual(Array(4).fill("[1] TRUE"))
})

test("compiled lazy calls and inherited method specificity work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL("../../crates/r-embed/tests/fixtures/gnu-bytecode-calls/identity-promise.rds", import.meta.url))
  const stream = Buffer.alloc(16)
  ;[12,20,0,1].forEach((word,i) => stream.writeInt32BE(word,i*4))
  const offset=bytes.indexOf(stream)
  expect(offset).toBeGreaterThan(-1)
  bytes.writeInt32BE(16,offset+4)
  await page.goto("/console/")
  const output=await page.evaluate(async (values) => {
    const { RRuntime }=await import("/src/runtime/r-runtime.ts")
    const runtime=new RRuntime()
    try {
      const results=[]
      for (const code of [
        `f<-unserialize(as.raw(c(${values})));identical(f(41L),quote(x))`,
        "g<-unserialize(serialize(f,NULL));identical(g(99L),quote(x))",
        "invisible(capture.output(d<-compiler::disassemble(f)));identical(d[[2]][[2]],as.name('GETFUN.OP'))&&identical(d[[3]][[3]][[2]][[2]],as.name('LDCONST.OP'))",
        "local({setClass('SelectA');setClass('SelectB',contains='SelectA');setGeneric('selectprobe',function(x)standardGeneric('selectprobe'));setMethod('selectprobe','SelectA',function(x)42L);m<-selectMethod('selectprobe','SelectB');identical(m(new('SelectB')),42L)&&identical(as.character(m@defined),'SelectA')})",
        "local({setClass('A');setClass('B',contains='A');setClass('C',contains='B');setClass('D',contains=c('A','C'));setGeneric('f',function(x)standardGeneric('f'));setMethod('f','C',function(x)'C');setMethod('f','A',function(x)'A');identical(f(new('D')),'C')})",
        "identical(Re(fft(c(1,0,0,0))),rep(1,4))",
      ]) results.push((await runtime.run(code,"console")).output.trim())
      return results
    } finally { runtime.dispose() }
  },Array.from(bytes).join(","))
  expect(output).toEqual(Array(6).fill("[1] TRUE"))
})

test("GNU math bytecode, method continuation and abort discovery work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL(
    "../../crates/r-embed/tests/fixtures/gnu-red-compiler/sqrt.rds", import.meta.url
  ))
  const stream = Buffer.alloc(36)
  ;[12, 123, 0, 8, 20, 1, 49, 0, 1].forEach((word, i) => stream.writeInt32BE(word, i * 4))
  const offset = bytes.indexOf(stream)
  expect(offset).toBeGreaterThan(-1)
  bytes.writeInt32BE(50, offset + 6 * 4)
  await page.goto("/console/")
  const results = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const output = []
      for (const code of [
        `f<-unserialize(as.raw(c(${values})));identical(round(f(4),6),54.59815)`,
        "local({setGeneric('nextprobe',function(x)standardGeneric('nextprobe'));setMethod('nextprobe','ANY',function(x)'any');setMethod('nextprobe','numeric',function(x)paste('num',callNextMethod()));identical(nextprobe(1),'num any')})",
        "local({g<-function(n)if(n>0)Recall(n-1)else 42L;identical(g(3),42L)})",
        "withRestarts(identical(length(computeRestarts()),2L)&&is.environment(findRestart('probe')$exit),probe=function()1)",
      ]) output.push((await runtime.run(code,"console")).output.trim())
      return output
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(results).toEqual(Array(4).fill("[1] TRUE"))
})

test("unary GNU bytecode, primitive methods and restart unwinding work in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL(
    "../../crates/r-embed/tests/fixtures/gnu-bytecode-unary/negative.rds", import.meta.url
  ))
  const stream = Buffer.alloc(24)
  ;[12, 20, 1, 42, 0, 1].forEach((word, i) => stream.writeInt32BE(word, i * 4))
  const offset = bytes.indexOf(stream)
  expect(offset).toBeGreaterThan(-1)
  bytes.writeInt32BE(43, offset + 3 * 4)
  await page.goto("/console/")
  const result = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const code = `f<-unserialize(as.raw(c(${values})));identical(f(c(1L,NA_integer_)),c(1L,NA_integer_))`
      const results = []
      for (const expression of [code,
        "rep.foo<-function(x,...)deparse(substitute(x));x<-structure(1,class='foo');identical(rep(x,stop('unused')),'x')",
        "`<.foo`<-function(e1,e2)TRUE;isTRUE(structure(9,class='foo')<1)",
        "h<-function(...)rep(1L,...);identical(h(,length.out=3L),c(1L,1L,1L))",
        "trace<-character();g<-function(){on.exit({gc();trace<<-c(trace,'cleanup')});invokeRestart('outer',42L)};value<-withRestarts({withRestarts(g(),inner=function()0);99L},outer=function(x){trace<<-c(trace,'handler');x});identical(value,42L)&&identical(trace,c('cleanup','handler'))",
      ]) {
        results.push((await runtime.run(expression,"console")).output.trim())
      }
      return results
    } finally { runtime.dispose() }
  }, Array.from(bytes).join(","))
  expect(result).toEqual(Array(5).fill("[1] TRUE"))
})

test("GNU arithmetic bytecode and Ops conflict hooks run in Wasm", async ({ page }) => {
  const bytes = readFileSync(new URL(
    "../../crates/r-embed/tests/fixtures/gnu-bytecode-arithmetic/add.rds",
    import.meta.url
  ))
  const stream = Buffer.alloc(32)
  ;[12, 20, 1, 20, 2, 44, 0, 1].forEach((word, i) => stream.writeInt32BE(word, i * 4))
  const offset = bytes.indexOf(stream)
  expect(offset).toBeGreaterThan(-1)
  bytes.writeInt32BE(45, offset + 5 * 4) // SUB, retaining source x + y
  await page.goto("/console/")
  const result = await page.evaluate(async (values) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const arithmetic = await runtime.run(
        `f<-unserialize(as.raw(c(${values})));g<-unserialize(serialize(f,NULL));identical(g(c(8L,NA_integer_,10L),3L),c(5L,NA_integer_,7L))`,
        "console"
      )
      const dispatch = await runtime.run(
        "`+.foo`<-function(e1,e2)10L;`+.bar`<-function(e1,e2)20L;chooseOpsMethod.bar<-function(x,y,mx,my,cl,reverse){gc();TRUE};identical(structure(1,class='foo')+structure(2,class='bar'),20L)",
        "console"
      )
      return [arithmetic.output.trim(), dispatch.output.trim()]
    } finally {
      runtime.dispose()
    }
  }, Array.from(bytes).join(","))
  expect(result).toEqual(["[1] TRUE", "[1] TRUE"])
})

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

test("interpreted active bindings and caller-scoped grid methods work in Wasm", async ({
  page,
}) => {
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

test("S3 group methods and serialized call tails work in the Wasm worker", async ({
  page,
}) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      return await runtime.run(
        `
        Math.foo <- function(x, ...) structure(NextMethod(), class = class(x))
        Ops.foo <- function(e1, e2) structure(NextMethod(), class = "foo")
        Summary.foo <- function(..., na.rm = FALSE) NextMethod()
        Complex.foo <- function(z) structure(NextMethod(), class = class(z))
        x <- structure(c(-1, 2), class = "foo")
        z <- structure(1 + 2i, class = "foo")
        expr <- quote(f(1L, b = 2L))
        stopifnot(identical(class(abs(x)), "foo"), identical(class(x + 1), "foo"),
                  sum(x) == 1, identical(class(Re(z)), "foo"),
                  identical(expr, unserialize(serialize(expr, NULL))))
        Ops.expr <- function(e1,e2) deparse(substitute(e1))
        w <- structure(1,class="expr")
        stopifnot(identical(w + 1,"w"))
        inherited <- structure(c(1, 2), class="unhandled") + 1
        stopifnot(identical(class(inherited), "unhandled"))
        f <- function() { on.exit(gc()); return(c(4L, 5L)) }
        stopifnot(identical(f(), c(4L, 5L)))
        cat("group and serialization contracts passed")
      `,
        "console"
      )
    } finally {
      runtime.dispose()
    }
  })
  expect(result.output.trim()).toBe("group and serialization contracts passed")
})
