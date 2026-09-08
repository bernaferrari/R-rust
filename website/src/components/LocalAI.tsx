import { useEffect, useRef, useState } from "react"
import { Check, Code2, PlugZap, RotateCcw, Send, Square } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  DEFAULT_OLLAMA_URL,
  chatWithOllama,
  listOllamaModels,
} from "@/ai/local-ai"

type Props = { onUseCode: (code: string) => void }

export function LocalAI({ onUseCode }: Props) {
  const [prompt, setPrompt] = useState(
    "Plot a gentle sine wave with 40 points and add a smooth LOESS curve."
  )
  const [code, setCode] = useState("")
  const [url, setUrl] = useState(DEFAULT_OLLAMA_URL)
  const [models, setModels] = useState<string[]>([])
  const [model, setModel] = useState("")
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState("Connect Ollama to choose a model.")
  const [error, setError] = useState("")
  const aborter = useRef<AbortController | null>(null)
  const mounted = useRef(true)
  const operation = useRef(0)

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      operation.current += 1
      aborter.current?.abort()
    }
  }, [])

  async function connect() {
    const token = ++operation.current
    setBusy(true)
    setError("")
    setModels([])
    setModel("")
    aborter.current = new AbortController()
    setStatus("Checking your local Ollama server…")
    try {
      const next = await listOllamaModels(url, aborter.current.signal)
      if (!mounted.current || operation.current !== token) return
      setModels(next)
      setModel(next[0] ?? "")
      setStatus(
        next.length
          ? "Connected. Choose a model and draft."
          : "Connected, but Ollama has no models installed."
      )
    } catch (cause) {
      if (mounted.current && operation.current === token) {
        setError(
          cause instanceof Error
            ? cause.message
            : "Could not connect to Ollama."
        )
        setStatus("Ollama is disconnected.")
      }
    } finally {
      if (mounted.current && operation.current === token) {
        setBusy(false)
        aborter.current = null
      }
    }
  }

  async function draft() {
    if (!prompt.trim()) {
      setError("Describe the R code you want drafted first.")
      return
    }
    if (!model) {
      setError("Connect Ollama and choose a model first.")
      return
    }
    const token = ++operation.current
    setBusy(true)
    setError("")
    aborter.current = new AbortController()
    setStatus("Drafting code…")
    try {
      const next = await chatWithOllama(
        url,
        model,
        prompt,
        aborter.current.signal
      )
      if (mounted.current && operation.current === token) {
        setCode(next)
        setStatus("Draft ready for review.")
      }
    } catch (cause) {
      if (
        mounted.current &&
        operation.current === token &&
        (cause as Error).name !== "AbortError"
      ) {
        setError(cause instanceof Error ? cause.message : "The draft failed.")
        setStatus("Ready when you are.")
      }
    } finally {
      if (mounted.current && operation.current === token) {
        setBusy(false)
        aborter.current = null
      }
    }
  }

  function stop() {
    operation.current += 1
    aborter.current?.abort()
    setBusy(false)
    setStatus("Draft stopped.")
  }

  return (
    <section className="local-ai" aria-labelledby="local-ai-title">
      <div className="ai-kicker">Local code studio</div>
      <h2
        id="local-ai-title"
        className="mt-2 text-2xl font-semibold tracking-tight"
      >
        A code companion
      </h2>
      <p className="ai-muted mt-2 max-w-2xl">
        Ollama drafts the R code on your computer. Review it, then open it in
        the playground.
      </p>
      <div className="ai-grid">
        <div className="ai-panel ai-compose">
          <label className="ai-label" htmlFor="ollama-url">
            Ollama URL
          </label>
          <div className="ai-row">
            <input
              id="ollama-url"
              className="ai-input"
              style={{ flex: 1, minHeight: 0 }}
              value={url}
              onChange={(event) => setUrl(event.target.value)}
              disabled={busy}
              spellCheck={false}
            />
            <Button
              type="button"
              variant="outline"
              onClick={() => void connect()}
              disabled={busy}
            >
              <PlugZap /> Connect
            </Button>
          </div>
          <label className="ai-label mt-4" htmlFor="ollama-model">
            Model
          </label>
          <Select
            value={model}
            onValueChange={(value) => setModel(value ?? "")}
            disabled={busy || !models.length}
          >
            <SelectTrigger
              id="ollama-model"
              className="ai-select w-full min-w-0"
            >
              <SelectValue
                placeholder={
                  models.length ? "Choose a model" : "Connect to list models"
                }
              />
            </SelectTrigger>
            <SelectContent>
              {models.map((name) => (
                <SelectItem key={name} value={name}>
                  {name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="ai-setup-note">
            If the connection is blocked, allow this site with{" "}
            <code>OLLAMA_ORIGINS</code>.{" "}
            <a
              href="https://docs.ollama.com/faq"
              target="_blank"
              rel="noreferrer"
            >
              Setup guide ↗
            </a>
          </p>
          <div className="ai-prompt-area">
            <label className="ai-label" htmlFor="r-prompt">
              What would you like to explore?
            </label>
            <textarea
              id="r-prompt"
              className="ai-input"
              value={prompt}
              onChange={(event) =>
                setPrompt(event.target.value.slice(0, 12000))
              }
              maxLength={12000}
              spellCheck={false}
            />
            <div className="ai-row mt-3">
              <Button
                type="button"
                onClick={() => void draft()}
                disabled={busy || !model}
              >
                <Send /> Draft code
              </Button>
              {busy && (
                <Button type="button" variant="outline" onClick={stop}>
                  <Square /> Stop
                </Button>
              )}
            </div>
            <p className="ai-muted ai-status" role="status">
              {status}
            </p>
            {error && (
              <p className="ai-error mt-3" role="alert">
                {error}
              </p>
            )}
          </div>
        </div>
        <div className={"ai-panel ai-draft " + (!code ? "is-empty" : "")}>
          <div className="ai-row ai-draft-heading justify-between">
            <div>
              <div className="ai-label mb-1">
                <Code2 size={16} /> Your R code
              </div>
              <p className="ai-muted">Edit freely before use.</p>
            </div>
            {code && (
              <Check aria-label="Draft ready" className="draft-ready-icon" />
            )}
          </div>
          {!code && (
            <div className="ai-draft-empty">
              <strong>Review before you run.</strong>
              <p>
                Connect Ollama, describe your idea, and your editable draft will
                appear here.
              </p>
            </div>
          )}
          {code && (
            <textarea
              className="ai-code mt-4"
              aria-label="Editable R code draft"
              value={code}
              onChange={(event) => setCode(event.target.value)}
              spellCheck={false}
            />
          )}
          {code && (
            <div className="ai-row mt-3">
              <Button type="button" onClick={() => onUseCode(code)}>
                <Check /> Run in playground
              </Button>
              <Button type="button" variant="ghost" onClick={() => setCode("")}>
                <RotateCcw /> Clear
              </Button>
            </div>
          )}
        </div>
      </div>
    </section>
  )
}
