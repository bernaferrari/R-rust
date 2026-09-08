export const DEFAULT_OLLAMA_URL = "http://localhost:11434"

export const LOCAL_R_SYSTEM_PROMPT = `You draft code for Rport, a portable R runtime. Return only useful R code in a fenced \`r block or plain code, with no explanation. Use base R plus the supported loess and grid surfaces. Do not assume ggplot2 or other packages are installed. Keep code deterministic, explicit, and safe to review. Never suggest executing arbitrary shell commands or downloading packages.`

export function extractRCode(raw: string): string {
  const fenced = raw.match(/```([A-Za-z0-9_+-]*)\s*\n?([\s\S]*?)```/)
  if (fenced && fenced[1].toLowerCase() !== "r")
    throw new Error("The model returned a non-R code block.")
  const candidate = (fenced?.[2] ?? raw).trim()
  if (!candidate) throw new Error("The model returned no R code to review.")
  if (candidate.length > 20_000)
    throw new Error("The generated code is too large to review safely.")
  if (
    /^(?:here(?:'s| is)|sure[,!]|explanation\s*:|the following|i can)/i.test(
      candidate
    )
  ) {
    throw new Error("The model returned explanation instead of plain R code.")
  }
  return candidate
}

export function normalizeOllamaUrl(value: string): string {
  const trimmed = value.trim()
  if (!trimmed) return DEFAULT_OLLAMA_URL
  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    throw new Error("Ollama URL must be a valid localhost http(s) URL.")
  }
  if (
    !/^https?:$/.test(parsed.protocol) ||
    !["localhost", "127.0.0.1", "[::1]", "::1"].includes(parsed.hostname) ||
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash ||
    (parsed.pathname !== "/" && parsed.pathname !== "")
  ) {
    throw new Error(
      "Ollama URL must use http(s) on localhost, 127.0.0.1, or [::1], without a path or credentials."
    )
  }
  return parsed.toString().replace(/\/$/, "")
}

export async function listOllamaModels(
  baseUrl: string,
  signal?: AbortSignal
): Promise<string[]> {
  const response = await fetch(`${normalizeOllamaUrl(baseUrl)}/api/tags`, {
    signal,
  })
  if (!response.ok)
    throw new Error(`Ollama returned ${response.status} while listing models.`)
  const data = (await response.json()) as { models?: Array<{ name?: string }> }
  return (data.models ?? [])
    .map((model) => model.name)
    .filter((name): name is string => Boolean(name))
}

export async function chatWithOllama(
  baseUrl: string,
  model: string,
  prompt: string,
  signal?: AbortSignal
): Promise<string> {
  if (!model.trim()) throw new Error("Choose an Ollama model first.")
  const response = await fetch(`${normalizeOllamaUrl(baseUrl)}/api/chat`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    signal,
    body: JSON.stringify({
      model: model.trim(),
      stream: false,
      messages: [
        { role: "system", content: LOCAL_R_SYSTEM_PROMPT },
        { role: "user", content: prompt.slice(0, 12_000) },
      ],
    }),
  })
  if (!response.ok)
    throw new Error(`Ollama returned ${response.status} while drafting code.`)
  const data = (await response.json()) as { message?: { content?: string } }
  return extractRCode(data.message?.content ?? "")
}
