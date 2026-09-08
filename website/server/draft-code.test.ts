import { describe, expect, it, vi } from "vitest"
import { configuredDraftHandler, createDraftHandler } from "./draft-code"

function setup() {
  const authenticate = vi.fn(async () => "verified-user")
  const reserve = vi.fn(async () => true)
  const generate = vi.fn(async () => "```r\nplot(1:10)\n```")
  const handler = createDraftHandler({
    origin: "https://rove.test",
    authenticate,
    reserve,
    generate,
  })
  const request = (
    body: unknown = { prompt: "Plot ten points" },
    headers: Record<string, string> = {}
  ) =>
    new Request("https://rove.test/api/draft", {
      method: "POST",
      headers: {
        origin: "https://rove.test",
        "content-type": "application/json",
        authorization: "Bearer test",
        ...headers,
      },
      body: JSON.stringify(body),
    })
  return { authenticate, reserve, generate, handler, request }
}

describe("paid R drafting boundary", () => {
  it("verifies identity and reserves quota before making a paid call", async () => {
    const s = setup()
    const response = await s.handler(s.request())
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual({ code: "plot(1:10)" })
    expect(s.reserve).toHaveBeenCalledWith("verified-user", expect.any(Number))
    expect(s.authenticate.mock.invocationCallOrder[0]).toBeLessThan(
      s.reserve.mock.invocationCallOrder[0]
    )
    expect(s.reserve.mock.invocationCallOrder[0]).toBeLessThan(
      s.generate.mock.invocationCallOrder[0]
    )
  })
  it("rejects caller-supplied identities without a valid token", async () => {
    const s = setup()
    s.authenticate.mockRejectedValueOnce(new Error("bad token"))
    expect(
      (await s.handler(s.request({ prompt: "plot", user: "admin" }))).status
    ).toBe(401)
    expect(s.reserve).not.toHaveBeenCalled()
    expect(s.generate).not.toHaveBeenCalled()
  })
  it("does not charge when origin or input validation fails", async () => {
    const s = setup()
    expect(
      (await s.handler(s.request({}, { origin: "https://other.test" }))).status
    ).toBe(403)
    expect(
      (await s.handler(s.request({ prompt: "x".repeat(9000) }))).status
    ).toBe(413)
    expect((await s.handler(s.request({ prompt: "" }))).status).toBe(400)
    expect(s.reserve).not.toHaveBeenCalled()
    expect(s.generate).not.toHaveBeenCalled()
  })
  it("fails closed when the shared quota store fails or denies", async () => {
    const s = setup()
    s.reserve.mockRejectedValueOnce(new Error("redis offline"))
    expect((await s.handler(s.request())).status).toBe(503)
    s.reserve.mockResolvedValueOnce(false)
    expect((await s.handler(s.request())).status).toBe(429)
    expect(s.generate).not.toHaveBeenCalled()
  })
  it("does not leak provider errors or accept non-R responses", async () => {
    const s = setup()
    s.generate.mockRejectedValueOnce(new Error("secret-key=private"))
    const error = await s.handler(s.request())
    expect(error.status).toBe(502)
    expect(await error.text()).not.toContain("private")
    s.generate.mockResolvedValueOnce("```python\nprint(1)\n```")
    expect((await s.handler(s.request())).status).toBe(502)
  })
  it("stays disabled without all deployment configuration", async () => {
    expect((await configuredDraftHandler({})(new Request("https://rove.test/api/draft"))).status).toBe(503)
  })
})
