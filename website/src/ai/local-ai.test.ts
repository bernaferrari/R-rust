import { describe, expect, it } from "vitest"
import { extractRCode, normalizeOllamaUrl } from "./local-ai"

describe("local AI service validation", () => {
  it("accepts only local Ollama origins", () => {
    expect(normalizeOllamaUrl("http://localhost:11434/")).toBe(
      "http://localhost:11434"
    )
    expect(normalizeOllamaUrl("http://127.0.0.1:11434")).toBe(
      "http://127.0.0.1:11434"
    )
    expect(() => normalizeOllamaUrl("https://example.com")).toThrow()
    expect(() => normalizeOllamaUrl("http://localhost:11434/api")).toThrow()
    expect(() =>
      normalizeOllamaUrl("http://user:pass@localhost:11434")
    ).toThrow()
  })

  it("extracts R fences and rejects other languages or prose", () => {
    expect(extractRCode("```r\nx <- seq(0, 1)\n```")).toBe("x <- seq(0, 1)")
    expect(extractRCode("x <- 1\nplot(x)")).toBe("x <- 1\nplot(x)")
    expect(() => extractRCode("```python\nprint('nope')\n```")).toThrow()
    expect(() => extractRCode("Here is your code:\nx <- 1")).toThrow()
  })
})
