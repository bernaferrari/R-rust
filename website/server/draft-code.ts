import { createHash } from "node:crypto"
import { createDeepSeek } from "@ai-sdk/deepseek"
import { generateText } from "ai"
import { createRemoteJWKSet, jwtVerify } from "jose"
import { extractRCode, LOCAL_R_SYSTEM_PROMPT } from "../src/ai/local-ai"

const MAX_BODY_BYTES = 8_192
const MAX_PROMPT_BYTES = 4_096
const MAX_OUTPUT_TOKENS = 2_048

export type DraftServices = {
  origin: string
  authenticate: (token: string) => Promise<string>
  reserve: (user: string, tokens: number) => Promise<boolean>
  generate: (prompt: string, signal: AbortSignal) => Promise<string>
}
const json = (status: number, body: Record<string, unknown>) =>
  Response.json(body, { status, headers: { "Cache-Control": "no-store" } })

/** Authentication and atomic quota reservation precede every paid call. */
export function createDraftHandler(services: DraftServices) {
  return async (request: Request): Promise<Response> => {
    if (request.method !== "POST") return json(405, { error: "Use POST." })
    if (request.headers.get("origin") !== services.origin)
      return json(403, { error: "This origin is not allowed." })
    if (
      request.headers.get("content-type")?.split(";")[0].trim() !==
      "application/json"
    )
      return json(415, { error: "Send a JSON request." })
    const token = request.headers
      .get("authorization")
      ?.match(/^Bearer (\S+)$/)?.[1]
    if (!token) return json(401, { error: "Sign in to draft R code." })
    let user: string
    try {
      user = await services.authenticate(token)
    } catch {
      return json(401, { error: "Your sign-in expired. Sign in again." })
    }
    if (!user) return json(401, { error: "Sign in to draft R code." })

    let prompt: string
    try {
      const reader = request.body?.getReader()
      if (!reader)
        return json(400, { error: "Describe what you want to explore." })
      const chunks: Uint8Array[] = []
      let size = 0
      try {
        while (true) {
          const part = await reader.read()
          if (part.done) break
          size += part.value.byteLength
          if (size > MAX_BODY_BYTES) {
            await reader.cancel()
            return json(413, { error: "Keep your description shorter." })
          }
          chunks.push(part.value)
        }
      } finally {
        reader.releaseLock()
      }
      const raw = Buffer.concat(chunks)
      const input = JSON.parse(raw.toString("utf8")) as { prompt?: unknown }
      if (typeof input.prompt !== "string" || !input.prompt.trim())
        return json(400, { error: "Describe what you want to explore." })
      prompt = input.prompt.trim()
      if (Buffer.byteLength(prompt) > MAX_PROMPT_BYTES)
        return json(413, { error: "Keep your description shorter." })
    } catch {
      return json(400, { error: "The request could not be read." })
    }

    // UTF-8 bytes conservatively bound ordinary text tokens; allow extra
    // message framing tokens. No tool calls, hidden user-supplied context or retries.
    const reservation =
      Buffer.byteLength(LOCAL_R_SYSTEM_PROMPT + prompt) +
      MAX_OUTPUT_TOKENS +
      256
    try {
      if (!(await services.reserve(user, reservation)))
        return json(429, {
          error: "The drafting limit has been reached. Try again later.",
        })
    } catch {
      return json(503, { error: "Drafting is temporarily unavailable." })
    }
    try {
      const signal = AbortSignal.any([
        request.signal,
        AbortSignal.timeout(20_000),
      ])
      const raw = await services.generate(prompt, signal)
      const code = extractRCode(raw)
      return json(200, { code })
    } catch {
      return json(502, {
        error: "The draft could not be generated. Please try again.",
      })
    }
  }
}

// One transaction for account minute/day and global daily token reservations.
// Reservations remain consumed on failure, preventing retry-based budget bypass.
const RESERVE = `
local minute = tonumber(redis.call('GET', KEYS[1]) or '0')
local day = tonumber(redis.call('GET', KEYS[2]) or '0')
local tokens = tonumber(redis.call('GET', KEYS[3]) or '0')
if minute >= 3 or day >= 10 or tokens + tonumber(ARGV[1]) > tonumber(ARGV[2]) then return 0 end
redis.call('INCR', KEYS[1]); redis.call('EXPIRE', KEYS[1], 120)
redis.call('INCR', KEYS[2]); redis.call('EXPIRE', KEYS[2], 172800)
redis.call('INCRBY', KEYS[3], ARGV[1]); redis.call('EXPIRE', KEYS[3], 172800)
return 1
`

/** Requires configured identity verification and shared Redis; never falls back to memory. */
export function configuredDraftHandler(
  env: Record<string, string | undefined>
) {
  const required = [
    "DEEPSEEK_API_KEY",
    "AI_ALLOWED_ORIGIN",
    "AI_JWKS_URL",
    "AI_JWT_ISSUER",
    "AI_JWT_AUDIENCE",
    "AI_REDIS_URL",
    "AI_REDIS_TOKEN",
    "AI_DAILY_TOKEN_BUDGET",
  ] as const
  if (required.some((key) => !env[key]))
    return async () =>
      json(503, { error: "AI drafting is not configured yet." })
  const budget = Number(env.AI_DAILY_TOKEN_BUDGET)
  if (!Number.isSafeInteger(budget) || budget <= 0)
    throw new Error("AI_DAILY_TOKEN_BUDGET must be a positive safe integer")
  const jwksUrl = new URL(env.AI_JWKS_URL!)
  const redisUrl = new URL(env.AI_REDIS_URL!)
  if (jwksUrl.protocol !== "https:" || redisUrl.protocol !== "https:")
    throw new Error("AI identity and quota services require HTTPS")
  const keys = createRemoteJWKSet(jwksUrl)
  const provider = createDeepSeek({ apiKey: env.DEEPSEEK_API_KEY! })
  return createDraftHandler({
    origin: new URL(env.AI_ALLOWED_ORIGIN!).origin,
    async authenticate(token) {
      const { payload } = await jwtVerify(token, keys, {
        issuer: env.AI_JWT_ISSUER!,
        audience: env.AI_JWT_AUDIENCE!,
        requiredClaims: ["sub", "exp"],
        algorithms: ["RS256", "ES256"],
      })
      if (!payload.sub) throw new Error("Missing subject")
      return createHash("sha256")
        .update(`${payload.iss}\0${payload.sub}`)
        .digest("hex")
    },
    async reserve(user, tokens) {
      const now = Date.now()
      const response = await fetch(redisUrl, {
        method: "POST",
        signal: AbortSignal.timeout(3_000),
        headers: {
          Authorization: `Bearer ${env.AI_REDIS_TOKEN}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify([
          "EVAL",
          RESERVE,
          3,
          `r-draft:${user}:minute:${Math.floor(now / 60_000)}`,
          `r-draft:${user}:day:${Math.floor(now / 86_400_000)}`,
          `r-draft:tokens:${Math.floor(now / 86_400_000)}`,
          tokens,
          budget,
        ]),
      })
      if (!response.ok) throw new Error("Quota service unavailable")
      const body = (await response.json()) as {
        result?: unknown
        error?: unknown
      }
      if (body.error || (body.result !== 0 && body.result !== 1))
        throw new Error("Invalid quota response")
      return body.result === 1
    },
    async generate(prompt, signal) {
      const result = await generateText({
        model: provider("deepseek-v4-flash"),
        system: LOCAL_R_SYSTEM_PROMPT,
        prompt,
        maxOutputTokens: MAX_OUTPUT_TOKENS,
        maxRetries: 0,
        abortSignal: signal,
        providerOptions: { deepseek: { thinking: { type: "disabled" } } },
      })
      return result.text
    },
  })
}
