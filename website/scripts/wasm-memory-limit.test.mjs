import { test } from "node:test"
import { strict as assert } from "node:assert"
import { assertBoundedMemory } from "./wasm-memory-limit.mjs"
const header = [0, 97, 115, 109, 1, 0, 0, 0]
const module = (...section) => Uint8Array.from([...header, ...section])
test("accepts a 256 MiB declared maximum", () =>
  assert.doesNotThrow(() =>
    assertBoundedMemory(module(5, 5, 1, 1, 1, 128, 32))
  ))
test("rejects unlimited and oversized imported artifacts", () => {
  assert.throws(() => assertBoundedMemory(module(5, 3, 1, 0, 1)))
  assert.throws(() => assertBoundedMemory(module(5, 5, 1, 1, 1, 129, 32)))
  assert.throws(() => assertBoundedMemory(module()))
})
test("rejects malformed memory sections", () => {
  assert.throws(() => assertBoundedMemory(module(5, 5, 1, 1, 1, 128)))
  assert.throws(() => assertBoundedMemory(module(5, 4, 1, 1, 2, 1)))
})
