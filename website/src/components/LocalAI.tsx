import { useEffect, useRef, useState } from "react"
import {
  Check,
  Cpu,
  Code2,
  Sparkles,
  Download,
  PlugZap,
  RotateCcw,
  Send,
  Square,
  Trash2,
} from "lucide-react"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
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
  chatWithWebEngine,
  createWebEngine,
  listOllamaModels,
} from "@/ai/local-ai"

type Props = { onUseCode: (code: string) => void }
type Source = "browser" | "ollama"

export function LocalAI({ onUseCode }: Props) {
  const [source, setSource] = useState<Source>("browser")
  const [prompt, setPrompt] = useState(
    "Plot a gentle sine wave with 40 points and add a smooth LOESS curve."
  )
  const [code, setCode] = useState("")
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState("")
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
        <div className="ai-panel ai-compose">
          <Tabs
            className="ai-model-card"
            value={source}
            onValueChange={(value) => setSource(value as Source)}
          >
            <TabsList className="ai-source-tabs" aria-label="Model source">
              <TabsTrigger value="browser" disabled={busy}>
                <Cpu /> In this browser
              </TabsTrigger>
              <TabsTrigger value="ollama" disabled={busy}>
                <PlugZap /> Ollama
              </TabsTrigger>
            </TabsList>
            <TabsContent value="browser">
              <div className="ai-model-heading">
                <span className="ai-model-icon">
                  <Cpu size={20} />
                </span>
                <div>
                  <strong>Qwen 2.5 Coder</strong>
                  <span>0.5B parameters · WebGPU</span>
                </div>
              </div>
              <p className="ai-muted ai-download-note">
                One-time download of several hundred MB from Hugging Face. Runs
                on your device.
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
            </TabsContent>
            <TabsContent value="ollama">
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
                    className="min-w-0 truncate"
                    placeholder={
                      models.length
                        ? "Choose a model"
                        : "Connect to list models"
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
            </TabsContent>
          </Tabs>
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
              <span>
                <Sparkles size={24} />
              </span>
              <strong>From an idea to a little R.</strong>
              <p>
                Load a model, describe your idea,
                <br />
                and your editable draft will appear here.
              </p>
            </div>
          )}
          {code && (
            <textarea
              className="ai-code mt-4"
              aria-label="Editable R code draft"
              value={code}
              onChange={(event) => setCode(event.target.value)}
              placeholder="Your R code"
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
