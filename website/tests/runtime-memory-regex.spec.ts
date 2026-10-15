import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

const fixture = (name: string, suffix = "R") =>
  readFileSync(
    new URL(
      `../../crates/r-embed/tests/fixtures/${name}.${suffix}`,
      import.meta.url
    ),
    "utf8"
  )

test("Unicode regex and immutable code values match GNU in the worker", async ({
  page,
}) => {
  await page.goto("/console/")
  const literalBytes = Array.from(
    readFileSync(
      new URL(
        "../../crates/r-embed/tests/fixtures/gnu-bytecode-string-literals.rds",
        import.meta.url
      )
    )
  )
  const result = await page.evaluate(
    async ({ contracts, literalBytes }) => {
      const { RRuntime } = await import("/src/runtime/r-runtime.ts")
      const runtime = new RRuntime()
      try {
        await runtime.importFile("literals.rds", Uint8Array.from(literalBytes))
        await runtime.run(
          "functions<-readRDS('literals.rds');invisible(NULL)",
          "console"
        )
        const outputs = []
        for (const code of contracts)
          outputs.push((await runtime.run(code, "console")).output)
        return outputs
      } finally {
        runtime.dispose()
      }
    },
    {
      literalBytes,
      contracts: [
        fixture("bytecode-string-cow-public-contract"),
        fixture("logical-singletons-public-contract"),
        fixture("regex-unicode-public-contract"),
      ],
    }
  )
  expect(result).toEqual(["[1] TRUE\n", "[1] TRUE\n", "[1] TRUE\n"])
})

for (const setup of ["fresh", "warm", "complete"] as const) {
  const warm = setup !== "fresh"
  for (const generation of [0, 1]) {
    test(`original S4 continuation fits the worker budget: ${setup} generation ${generation}`, async ({
      page,
    }) => {
      // The complete setup runs 31 separate original workflows. Each worker
      // request retains its unchanged 15s, 64MiB and 500000-node limits.
      if (setup === "complete") test.setTimeout(120_000)
      const names = [
        "complex-print-public-contract",
        "stats-namespace-public-contract",
        "get-lazy-mode-public-contract",
        "correlation-public-contract",
        "palette-public-contract",
        "tempfile-public-contract",
        "bincode-public-contract",
        "print-gap-public-contract",
        "arima0-public-contract",
        "utils-console-public-contract",
        "mapply-public-contract",
        "sample-condition-contract",
        "rep-len-admission-contract",
        "calling-error-handler-public-contract",
        "calling-warning-handler-public-contract",
      ]
      if (setup === "complete")
        names.push(
          "s3-methods-public-contract",
          "namespace-s3-startup-contract",
          "base-print-methods-public-contract",
          "compiled-dots-public-contract",
          "vector-print-limits-public-contract",
          "print-digits-public-contract",
          "prmatrix-public-contract",
          "condition-handler-stack-public-contract",
          "attribute-print-public-contract"
        )
      const preceding = warm ? names.map((name) => fixture(name)) : []
      if (setup === "complete")
        preceding.unshift(
          ...[
            "260_length_replacement",
            "492_rep_int_len_helpers",
            "288_with_calling_handlers",
            "388_no_print_summary_method_aliases",
            "1094_cat_complex_fft",
            "544_complex_constructor_format_str_parity",
            "551_round_signif_complex_parity",
          ].map((name) =>
            readFileSync(
              new URL(
                `../../tests/conformance/cases/${name}.R`,
                import.meta.url
              ),
              "utf8"
            )
          )
        )
      await page.goto("/console/")
      const results = await page.evaluate(
        async ({ prefix, original, generation }) => {
          const { RRuntime } = await import("/src/runtime/r-runtime.ts")
          const runtime = new RRuntime()
          try {
            if (generation) {
              await runtime.run(
                "setClass('NextBaseProbe',representation(a='numeric')); reset_fixture_marker <- TRUE; invisible(NULL)",
                "console"
              )
              runtime.reset()
              const empty = await runtime.run(
                "!exists('reset_fixture_marker', inherits=FALSE)",
                "console"
              )
              if (empty.output !== "[1] TRUE\n")
                throw new Error("reset retained the previous session")
            }
            const prefixMs = []
            for (const [index, code] of prefix.entries()) {
              const started = performance.now()
              try {
                await runtime.run(code, "console")
              } catch (error) {
                throw new Error(`S4 setup prefix ${index}: ${String(error)}`)
              }
              prefixMs.push(performance.now() - started)
            }
            const started = performance.now()
            const result = await runtime.run(original, "console")
            const elapsedMs = performance.now() - started
            const recovery = await runtime.run("1+1", "console")
            return {
              outputs: [result.output, recovery.output],
              elapsedMs,
              prefixMs,
              generation: runtime.sessionGeneration,
            }
          } finally {
            runtime.dispose()
          }
        },
        {
          prefix: preceding,
          original: fixture("s4-next-method-public-contract"),
          generation,
        }
      )
      expect(results.outputs).toEqual([
        fixture("s4-next-method-public-contract", "out"),
        "[1] 2\n",
      ])
      expect(results.generation).toBe(generation)
      console.log(
        JSON.stringify({
          s4: setup,
          generation,
          elapsedMs: results.elapsedMs,
          prefixMs: results.prefixMs,
        })
      )
    })
  }
}

test("canvas and compressed decode admission preserve the worker", async ({
  page,
}) => {
  await page.goto("/console/")
  const codecInputs = [
    "gzip.rds",
    "bzip2.rds",
    "xz.rds",
    "oversized-dictionary.xz",
  ].map((name) => ({
    name,
    bytes: Array.from(
      readFileSync(
        new URL(
          `../../crates/r-embed/tests/fixtures/native-codec-admission/${name}`,
          import.meta.url
        )
      )
    ),
  }))
  const result = await page.evaluate(async (inputs) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      for (const input of inputs)
        await runtime.importFile(input.name, Uint8Array.from(input.bytes))
      const generation = runtime.sessionGeneration
      let dictionaryRejected = false
      try {
        await runtime.run("readRDS('oversized-dictionary.xz')", "console")
      } catch {
        dictionaryRejected = true
      }
      await runtime.run("held <- list(); invisible(NULL)", "console")
      // Fill against the public budget, so the test remains independent of
      // bootstrap size and the renderer's conservative buffer estimate.
      let chunks = 0
      for (; chunks < 64; chunks++) {
        try {
          await runtime.run(
            "held[[length(held)+1L]] <- raw(1048576L); invisible(NULL)",
            "console"
          )
        } catch {
          break
        }
      }
      let rejected = false
      try {
        await runtime.run("canvas_side_effect <- TRUE; plot(1)", "plot")
      } catch {
        rejected = true
      }
      let codecRejections = 0
      for (const name of ["gzip.rds", "bzip2.rds", "xz.rds"]) {
        try {
          await runtime.run(`readRDS('${name}')`, "console")
        } catch {
          codecRejections++
        }
      }
      const recovery = await runtime.run(
        "exists('canvas_side_effect', inherits=FALSE)",
        "console"
      )
      return {
        rejected,
        dictionaryRejected,
        codecRejections,
        filled: chunks > 0 && chunks < 64,
        sameGeneration: runtime.sessionGeneration === generation,
        output: recovery.output,
      }
    } finally {
      runtime.dispose()
    }
  }, codecInputs)

  expect(result).toEqual({
    rejected: true,
    dictionaryRejected: true,
    codecRejections: 3,
    filled: true,
    sameGeneration: true,
    output: "[1] FALSE\n",
  })
})
