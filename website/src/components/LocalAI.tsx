import { useEffect, useRef, useState } from "react"
import {
  Check,
  Cpu,
  Download,
  PlugZap,
  RotateCcw,
  Send,
  Square,
  Trash2,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DEFAULT_OLLAMA_URL,
  chatWithOllama,
  chatWithWebEngine,
  createWebEngine,
  listOllamaModels,
} from "@/ai/local-ai"

type Props = { onUseCode: (code: string) => void }
type Source = "browser" | "ollama"

export function LocalAI({ onUseCode }: Props) {
  const [source, setSource] = useState<Source>("browser")
  const [prompt, setPrompt] = useState(
    "Create x <- seq(0, 1, length.out = 40), y <- sin(6 * x), fit a loess curve, and plot the points with the fitted line."
  )
  const [code, setCode] = useState("")
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState(
    "Model stays unloaded until you choose Load model."
  )
  const [progress, setProgress] = useState(0)
  const [error, setError] = useState("")
  const [url, setUrl] = useState(DEFAULT_OLLAMA_URL)
  const [models, setModels] = useState<string[]>([])
  const [model, setModel] = useState("")
  const [modelReady, setModelReady] = useState(false)
  const [loadingModel, setLoadingModel] = useState(false)
  const engine = useRef<Awaited<ReturnType<typeof createWebEngine>> | null>(
    null
  )
  const aborter = useRef<AbortController | null>(null)
  const mounted = useRef(true)
  const operation = useRef(0)

  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      operation.current += 1
      aborter.current?.abort()
      void engine.current?.unload()
    }
  }, [])

  function current(token: number) {
    return mounted.current && operation.current === token
  }

  async function connectOllama() {
    const token = ++operation.current
    setError("")
    setBusy(true)
    setModels([])
    setModel("")
    aborter.current = new AbortController()
    setStatus("Checking your local Ollama server…")
    try {
      const nextModels = await listOllamaModels(
        url,
        AbortSignal.any([aborter.current.signal, AbortSignal.timeout(15_000)])
      )
      if (!current(token)) return
      setModels(nextModels)
      setModel((current) => current || nextModels[0] || "")
      setStatus(
        nextModels.length
          ? "Connected. Choose a model and draft."
          : "Connected, but Ollama has no models installed."
      )
    } catch (cause) {
      if (current(token)) {
        setError(
          `${cause instanceof Error ? cause.message : "Could not connect to Ollama."} Check that Ollama is running and allows browser requests.`
        )
        setStatus("Ollama is disconnected.")
      }
    } finally {
      if (current(token)) {
        setBusy(false)
        aborter.current = null
      }
    }
  }

  async function loadBrowserModel() {
    if (engine.current) {
      setStatus("Browser model is ready.")
      return
    }
    const token = ++operation.current
    setError("")
    setBusy(true)
    setLoadingModel(true)
    setProgress(0)
    setStatus("Loading the browser model…")
    try {
      const nextEngine = await createWebEngine((value, text) => {
        if (current(token)) {
          setProgress(value)
          setStatus(text)
        }
      })
      if (!current(token)) {
        await nextEngine.unload()
        return
      }
      engine.current = nextEngine
      setModelReady(true)
      setStatus("Browser model ready.")
    } catch (cause) {
      if (current(token)) {
        setError(
          cause instanceof Error
            ? cause.message
            : "The browser model could not load."
        )
        setStatus("Browser model unavailable.")
      }
    } finally {
      if (current(token)) {
        setBusy(false)
        setLoadingModel(false)
      }
    }
  }

  async function draft() {
    if (!prompt.trim()) {
      setError("Describe the R code you want drafted first.")
      return
    }
    const token = ++operation.current
    setError("")
    setBusy(true)
    aborter.current = new AbortController()
    setStatus("Drafting code…")
    try {
      const nextCode =
        source === "ollama"
          ? await chatWithOllama(url, model, prompt, aborter.current.signal)
          : await chatWithWebEngine(engine.current!, prompt)
      if (current(token)) {
        setCode(nextCode)
        setStatus("Draft ready for review.")
      }
    } catch (cause) {
      if (current(token) && (cause as Error).name !== "AbortError") {
        setError(cause instanceof Error ? cause.message : "The draft failed.")
        setStatus("Ready when you are.")
      }
    } finally {
      if (current(token)) {
        setBusy(false)
        aborter.current = null
      }
    }
  }

  async function stop() {
    operation.current += 1
    aborter.current?.abort()
    engine.current?.interruptGenerate()
    setBusy(false)
    setStatus("Draft stopped.")
  }

  async function unload() {
    setBusy(true)
    try {
      await engine.current?.unload()
    } finally {
      engine.current = null
      setModelReady(false)
      setBusy(false)
      setCode("")
      setStatus("Browser model unloaded.")
    }
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
        Describe an idea, review the R code, then open it in the playground.
      </p>
      <div className="ai-grid">
        <div className="ai-panel">
          <div className="ai-row mb-4" role="group" aria-label="Model source">
            <Button
              type="button"
              variant={source === "browser" ? "default" : "outline"}
              onClick={() => setSource("browser")}
              disabled={busy}
            >
              <Cpu /> Browser model
            </Button>
            <Button
              type="button"
              variant={source === "ollama" ? "default" : "outline"}
              onClick={() => setSource("ollama")}
              disabled={busy}
            >
              <PlugZap /> Ollama
            </Button>
          </div>
          {source === "browser" ? (
            <>
              <p className="ai-muted">
                Qwen 2.5 Coder · 0.5B · WebGPU. Loading the model downloads
                several hundred MB from Hugging Face. Inference then runs in
                this browser.
              </p>
              {busy && (
                <div className="mt-4" aria-live="polite">
                  <div className="ai-progress">
                    <span style={{ width: `${Math.round(progress * 100)}%` }} />
                  </div>
                  <p className="ai-muted mt-2">{status}</p>
                </div>
              )}
              <div className="ai-row mt-4">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void loadBrowserModel()}
                  disabled={busy || modelReady}
                >
                  <Download /> {modelReady ? "Model ready" : "Load model"}
                </Button>
                {modelReady && (
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={() => void unload()}
                    disabled={busy}
                  >
                    <Trash2 /> Unload
                  </Button>
                )}
              </div>
            </>
          ) : (
            <>
              <label className="ai-label" htmlFor="ollama-url">
                Ollama URL
              </label>
              <div className="ai-row">
                <input
                  id="ollama-url"
                  className="ai-input"
                  style={{ flex: 1, minHeight: 0 }}
                  value={url}
                  onChange={(event) => {
                    setUrl(event.target.value)
                    setModel("")
                    setModels([])
                  }}
                  disabled={busy}
                  spellCheck={false}
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void connectOllama()}
                  disabled={busy}
                >
                  <PlugZap /> Connect
                </Button>
              </div>
              <label className="ai-label mt-4" htmlFor="ollama-model">
                Model
              </label>
              <select
                id="ollama-model"
                className="ai-select"
                value={model}
                onChange={(event) => setModel(event.target.value)}
                disabled={busy || !models.length}
              >
                <option value="">
                  {models.length ? "Choose a model" : "Connect to list models"}
                </option>
                {models.map((name) => (
                  <option key={name} value={name}>
                    {name}
                  </option>
                ))}
              </select>
              <p className="ai-muted mt-3">
                Ollama must allow requests from this site. Configure{" "}
                <code>OLLAMA_ORIGINS</code> for this site if the browser blocks
                localhost.{" "}
                <a
                  href="https://docs.ollama.com/faq"
                  target="_blank"
                  rel="noreferrer"
                >
                  Setup guide ↗
                </a>
              </p>
            </>
          )}
          <div className="ai-divider" />
          <label className="ai-label" htmlFor="r-prompt">
            What should the R code do?
          </label>
          <textarea
            id="r-prompt"
            className="ai-input"
            value={prompt}
            onChange={(event) => setPrompt(event.target.value.slice(0, 12000))}
            maxLength={12000}
            spellCheck={false}
          />
          <div className="ai-row mt-3">
            <Button
              type="button"
              onClick={() => void draft()}
              disabled={busy || (source === "ollama" ? !model : !modelReady)}
            >
              <Send /> Draft code
            </Button>
            {busy && !loadingModel && (
              <Button
                type="button"
                variant="outline"
                onClick={() => void stop()}
              >
                <Square /> Stop
              </Button>
            )}
          </div>
          <p className="ai-muted mt-3" aria-live="polite">
            {status}
          </p>
          {error && (
            <p className="ai-error mt-3" role="alert">
              {error}
            </p>
          )}
        </div>
        <div className="ai-panel">
          <div className="ai-row justify-between">
            <div>
              <div className="ai-label mb-1">Review draft</div>
              <p className="ai-muted">Edit freely before use.</p>
            </div>
            {code && (
              <Check aria-label="Draft ready" className="draft-ready-icon" />
            )}
          </div>
          <textarea
            className="ai-code mt-4"
            aria-label="Editable R code draft"
            value={code}
            onChange={(event) => setCode(event.target.value)}
            placeholder="Your reviewed R code will appear here…"
            spellCheck={false}
          />
          {code && (
            <div className="ai-row mt-3">
              <Button type="button" onClick={() => onUseCode(code)}>
                <Check /> Use this code
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
