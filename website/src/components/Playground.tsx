import {
  lazy,
  Suspense,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react"
import {
  Play,
  Square,
  RotateCcw,
  Download,
  Copy,
  Check,
  ArrowUpRight,
  Terminal,
  ChartNoAxesCombined,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { RRuntime, type RuntimeMode, type RuntimeStatus } from "@/runtime"
import { examples, type Example } from "@/data/examples"
const CodeEditor = lazy(() => import("./CodeEditor"))
export type PlaygroundInput = {
  code: string
  mode: RuntimeMode
  exampleId?: string
  key: number | string
}
function saveFile(contents: BlobPart, mime: string, name: string) {
  const url = URL.createObjectURL(new Blob([contents], { type: mime }))
  const a = document.createElement("a")
  a.href = url
  a.download = name
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
export function Playground({
  input,
  onSelect,
}: {
  input: PlaygroundInput
  onSelect: (example: Example) => void
}) {
  const [code, setCode] = useState(input.code)
  const [mode, setMode] = useState<RuntimeMode>(input.mode)
  const [status, setStatus] = useState<RuntimeStatus>("reset")
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [output, setOutput] = useState("")
  const [png, setPng] = useState("")
  const [duration, setDuration] = useState<number>()
  const [feedback, setFeedback] = useState("")
  const editorReady = useSyncExternalStore(
    subscribeClient,
    () => true,
    () => false
  )
  const runtime = useRef<RRuntime | null>(null)
  const imageUrl = useRef("")
  const runId = useRef(0)
  useEffect(() => {
    runtime.current = new RRuntime({ onStatus: setStatus, timeoutMs: 20_000 })
    return () => {
      runtime.current?.dispose()
      URL.revokeObjectURL(imageUrl.current)
    }
  }, [])

  async function run() {
    if (busy || !runtime.current) return
    const id = ++runId.current
    setBusy(true)
    setError("")
    setFeedback("")
    try {
      const result = await runtime.current.run(code, mode)
      if (id !== runId.current) return
      setOutput(result.output)
      setDuration(result.durationMs)
      URL.revokeObjectURL(imageUrl.current)
      imageUrl.current = result.png
        ? URL.createObjectURL(
            new Blob([new Uint8Array(result.png)], { type: "image/png" })
          )
        : ""
      setPng(imageUrl.current)
    } catch (e) {
      if (id === runId.current)
        setError(e instanceof Error ? e.message : String(e))
    } finally {
      if (id === runId.current) setBusy(false)
    }
  }
  function reset() {
    ++runId.current
    runtime.current?.reset()
    setBusy(false)
    setOutput("")
    setPng("")
    setError("")
    setDuration(undefined)
    setFeedback("Session cleared")
    URL.revokeObjectURL(imageUrl.current)
    imageUrl.current = ""
  }
  async function copy() {
    try {
      await navigator.clipboard.writeText(code)
      setFeedback("Code copied")
    } catch {
      setFeedback("Clipboard unavailable. Select the code to copy it.")
    }
  }
  const preview =
    input.exampleId && input.mode === "plot"
      ? `${import.meta.env.BASE_URL}examples/${input.exampleId}.png`
      : ""
  return (
    <section id="playground" className="section playground-section">
      <div className="section-heading">
        <div>
          <span className="eyebrow">01 / THE PLAYGROUND</span>
          <h2>
            A little code.
            <br />
            <em>A lot of possibility.</em>
          </h2>
        </div>
        <p>
          Change a number. Break something. Try again.
          <br />
          Run it, see what happens, and make it your own.
        </p>
      </div>
      <div className="workbench">
        <div className="workbench-toolbar">
          <div className="workbench-label">
            <span
              className={"status-dot " + (status === "ready" ? "ready" : "")}
            />
            <strong>R playground</strong>
            <span className="runtime-label">
              {busy
                ? "R is working…"
                : status === "ready"
                  ? "Wasm ready"
                  : "Ready when you are"}
            </span>
          </div>
          <div className="toolbar-actions">
            <Button
              variant="ghost"
              className="icon-control"
              aria-label="Copy R code"
              onClick={copy}
            >
              {feedback === "Code copied" ? <Check /> : <Copy />}
            </Button>
            <Button
              variant="ghost"
              className="icon-control"
              aria-label="Reset R session"
              onClick={reset}
            >
              <RotateCcw />
            </Button>
            <Button
              className="run-button"
              onClick={busy ? reset : run}
              disabled={!code.trim()}
            >
              {busy ? <Square /> : <Play fill="currentColor" />}
              {busy ? "Stop & reset" : "Run code"}
            </Button>
          </div>
        </div>
        <div className="workbench-body">
          <div className="editor-pane">
            <div className="pane-topline">
              <span>
                <span className="r-file">R</span> experiment.R
              </span>
              <label className="recipe-select">
                Start with{" "}
                <select
                  aria-label="Choose an R example"
                  value={input.exampleId ?? ""}
                  onChange={(e) => {
                    const example = examples.find(
                      (x) => x.id === e.target.value
                    )
                    if (example) onSelect(example)
                  }}
                >
                  <option value="" disabled>
                    Custom code
                  </option>
                  {examples.map((x) => (
                    <option key={x.id} value={x.id}>
                      {x.title}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            {editorReady ? (
              <Suspense
                fallback={
                  <textarea
                    aria-label="R code editor"
                    className="editor-fallback"
                    value={code}
                    onChange={(e) => setCode(e.target.value)}
                  />
                }
              >
                <CodeEditor code={code} onChange={setCode} onRun={run} />
              </Suspense>
            ) : (
              <textarea
                className="editor-fallback"
                aria-label="R code editor"
                value={code}
                onChange={(e) => setCode(e.target.value)}
              />
            )}
            <div className="editor-footer">
              <span>R · UTF-8</span>
              <span>⌘ / Ctrl + Enter to run</span>
            </div>
          </div>
          <div className="output-pane">
            <div className="pane-topline">
              <div
                className="output-tabs"
                role="group"
                aria-label="Execution mode"
              >
                <button
                  aria-pressed={mode === "plot"}
                  onClick={() => setMode("plot")}
                >
                  <ChartNoAxesCombined size={14} />
                  Plot
                </button>
                <button
                  aria-pressed={mode === "console"}
                  onClick={() => setMode("console")}
                >
                  <Terminal size={14} />
                  Console
                </button>
              </div>
              <Button
                variant="ghost"
                className="icon-control"
                aria-label={
                  mode === "plot" ? "Download plot" : "Download output"
                }
                disabled={mode === "plot" ? !png : !output}
                onClick={() => {
                  if (mode === "plot" && png) {
                    const a = document.createElement("a")
                    a.href = png
                    a.download = "plot.png"
                    a.click()
                  } else saveFile(output, "text/plain", "output.txt")
                }}
              >
                <Download />
              </Button>
            </div>
            <div className="result-area" aria-live="polite" aria-busy={busy}>
              {error ? (
                <div className="run-error" role="alert">
                  <strong>Something needs a tweak.</strong>
                  <pre>{error}</pre>
                  <p>Edit the code and try again.</p>
                </div>
              ) : mode === "plot" ? (
                png ? (
                  <img src={png} alt="Plot generated by your R code" />
                ) : preview ? (
                  <div className="preview-result">
                    <img
                      src={preview}
                      alt={`${examples.find((x) => x.id === input.exampleId)?.title} — R-generated preview`}
                    />
                    <span>Preview · run to update</span>
                  </div>
                ) : (
                  <div className="empty-result">
                    <ChartNoAxesCombined />
                    <p>Your next plot goes here.</p>
                    <span>Write some R and press Run code.</span>
                  </div>
                )
              ) : (
                <pre className="console-output">
                  {output ||
                    "# Your R output will appear here.\n# Try: mean(c(4, 8, 15, 16, 23, 42))"}
                </pre>
              )}
              {busy ? (
                <div className="working-note">
                  <span className="loading-dot" />
                  Running R on your device…
                </div>
              ) : null}
            </div>
            <div className="output-footer">
              <span>
                {duration !== undefined
                  ? `Completed in ${duration < 1000 ? duration.toFixed(0) + " ms" : (duration / 1000).toFixed(2) + " s"}`
                  : "800 × 600"}
              </span>
              <span>{mode === "plot" ? "PNG" : "R console"}</span>
            </div>
          </div>
        </div>
        <div className="workbench-bottom">
          <span role="status">{feedback}</span>
          <button onClick={() => saveFile(code, "text/plain", "experiment.R")}>
            Download .R <ArrowUpRight size={14} />
          </button>
        </div>
      </div>
      <p className="compat-note">
        An evolving Rust port of R. These examples use the supported runtime;
        arbitrary CRAN packages are not available.{" "}
        <a href="https://github.com/bernaferrari/R-rust/blob/main/docs/loess-and-portable-graphics.md">
          See compatibility notes ↗
        </a>
      </p>
    </section>
  )
}

function subscribeClient() {
  return () => {}
}
