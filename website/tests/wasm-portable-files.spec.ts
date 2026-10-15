import { test, expect } from "@playwright/test"
import { readFileSync } from "node:fs"

const anonymousContract = readFileSync(
  new URL(
    "../../crates/rmath/src/mainutils/connections/file/anonymous-contract.R",
    import.meta.url
  ),
  "utf8"
)

test("portable binary and anonymous files preserve records, visibility and cursor modes", async ({
  page,
}) => {
  await page.goto("/console/")
  const output = await page.evaluate(async (contract) => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    try {
      const values = []
      for (const code of [
        "{writeBin(c('a','b'),'records.bin');con<-file('records.bin','rb');zero<-readBin(con,'character',n=0L);first<-readBin(con,'character',n=1L);second<-readBin(con,'character',n=1L);close(con);identical(zero,character())&&identical(first,'a')&&identical(second,'b')}",
        "{x<-withVisible(writeBin(as.raw(c(1,2)),raw()));x$visible&&identical(x$value,as.raw(c(1,2)))}",
        "{x<-withVisible(writeBin(1L,'records.bin'));is.null(x$value)&&!x$visible}",
        `{${contract}\n}`,
      ])
        values.push((await runtime.run(code, "console")).output.trim())
      return values
    } finally {
      runtime.dispose()
    }
  }, anonymousContract)
  expect(output).toEqual(Array(4).fill("[1] TRUE"))
})

test("RDS filenames use isolated session files with portable compression", async ({
  page,
}) => {
  await page.goto("/console/")
  const result = await page.evaluate(async () => {
    const { RRuntime } = await import("/src/runtime/r-runtime.ts")
    const runtime = new RRuntime()
    const independent = new RRuntime()
    try {
      const values = []
      for (const code of [
        "{object<-list(text='R \u00e9',numbers=c(1,NA,Inf),raw=as.raw(c(0,128,255)));saveRDS(object,'object.rds');identical(readRDS('object.rds'),object)}",
        "{saveRDS(object,'plain.rds',compress=FALSE);identical(readRDS('plain.rds'),object)}",
        "{saveRDS(object,'ascii.rds',ascii=TRUE);identical(readRDS('ascii.rds'),object)}",
      ])
        values.push((await runtime.run(code, "console")).output.trim())
      const bytes = Array.from(await runtime.exportFile("object.rds"))
      await independent.importFile("imported.rds", Uint8Array.from(bytes))
      const imported = (
        await independent.run(
          "identical(readRDS('imported.rds')$raw,as.raw(c(0,128,255)))",
          "console"
        )
      ).output.trim()
      return {
        values,
        header: bytes.slice(0, 10),
        imported,
        files: await independent.listFiles(),
      }
    } finally {
      runtime.dispose()
      independent.dispose()
    }
  })
  expect(result.values).toEqual(Array(3).fill("[1] TRUE"))
  expect(result.header).toEqual([31, 139, 8, 0, 0, 0, 0, 0, 0, 3])
  expect(result.imported).toBe("[1] TRUE")
  expect(result.files).toEqual(["imported.rds"])
})
