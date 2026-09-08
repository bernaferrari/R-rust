import {
  lazy,
  Suspense,
  useEffect,
  useEffectEvent,
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
  Terminal,
  ChartNoAxesCombined,
} from "lucide-react"
import { toast } from "sonner"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { Button } from "@/components/ui/button"
import { RRuntime, type RuntimeMode } from "@/runtime"
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export function Playground({
  input,
  compact = false,
  onSelect,
  automatic,
  onAutomaticChange,
}: {
  compact?: boolean
  automatic: boolean
  onAutomaticChange: (value: boolean) => void
  input: PlaygroundInput
  onSelect: (example: Example) => void
}) {
  const [code, setCode] = useState(input.code)
  const [mode, setMode] = useState<RuntimeMode>(input.mode)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [output, setOutput] = useState("")
  const [png, setPng] = useState("")
  const [duration, setDuration] = useState<number>()
  const [feedback, setFeedback] = useState("")
  const [copied, setCopied] = useState(false)
  const copyTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const editorReady = useSyncExternalStore(
    subscribeClient,
    () => true,
    () => false
  )
  const runtime = useRef<RRuntime | null>(null)
  const imageUrl = useRef("")
  const runId = useRef(0)
  const autoTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  useEffect(() => {
    const generation = runId
    runtime.current = new RRuntime({ timeoutMs: 20_000 })
    return () => {
      ++generation.current
      clearTimeout(autoTimer.current)
      clearTimeout(copyTimer.current)
      runtime.current?.dispose()
      URL.revokeObjectURL(imageUrl.current)
    }
  }, [])

  async function run() {
    if (!runtime.current || !code.trim()) return
    clearTimeout(autoTimer.current)
    if (busy) runtime.current.reset()
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
  const autoRun = useEffectEvent(() => {
    void run()
  })
  useEffect(() => {
    if (!automatic || !code.trim()) return
    autoTimer.current = setTimeout(() => autoRun(), 650)
    return () => clearTimeout(autoTimer.current)
  }, [code, mode, automatic])

  function changeCode(next: string) {
    ++runId.current
    if (busy) runtime.current?.reset()
    setBusy(false)
    setCode(next)
    if (!next.trim()) {
      setPng("")
      setOutput("")
      setError("")
      setDuration(undefined)
    }
  }
  function changeMode(next: RuntimeMode) {
    ++runId.current
    if (busy) runtime.current?.reset()
    setBusy(false)
    setMode(next)
  }
  function reset() {
    clearTimeout(autoTimer.current)
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
      setCopied(true)
      toast.success("Copied")
      clearTimeout(copyTimer.current)
      copyTimer.current = setTimeout(() => {
        setCopied(false)
        setFeedback((current) => (current === "Code copied" ? "" : current))
      }, 1000)
    } catch {
      setFeedback("Clipboard unavailable. Select the code to copy it.")
    }
  }
  return (
    <section id="playground" className="section playground-section">
      {!compact && (
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
            The output updates as you edit.
          </p>
        </div>
      )}
      <div className="workbench">
        <div className="workbench-toolbar">
          <div className="workbench-label">
            <span className="playground-mark" aria-hidden="true">
              R
            </span>
            <div className="playground-title">
              <strong>Playground</strong>
            </div>
          </div>
          <div className="toolbar-actions">
            <Select
              value={automatic ? "auto" : "manual"}
              onValueChange={(value) => {
                if (value) {
                  clearTimeout(autoTimer.current)
                  onAutomaticChange(value === "auto")
                }
              }}
            >
              <SelectTrigger
                className="execution-select"
                aria-label="When to run code"
              >
                <SelectValue>
                  {automatic ? "Run automatically" : "Run manually"}
                </SelectValue>
              </SelectTrigger>
              <SelectContent
                className="execution-options"
                align="end"
                alignItemWithTrigger={false}
              >
                <SelectItem value="auto">
                  <span>
                    <strong>Run automatically</strong>
                    <small>Updates after you pause typing</small>
                  </span>
                </SelectItem>
                <SelectItem value="manual">
                  <span>
                    <strong>Run manually</strong>
                    <small>Only runs when you choose Run code</small>
                  </span>
                </SelectItem>
              </SelectContent>
            </Select>

            <Button
              variant="ghost"
              className="icon-control"
              aria-label="Reset R session"
              onClick={reset}
            >
              <RotateCcw />
            </Button>
            {(!automatic || busy) && (
              <Button
                className="run-button"
                onClick={busy ? reset : run}
                disabled={!code.trim()}
              >
                {busy ? <Square /> : <Play fill="currentColor" />}
                {busy ? "Stop" : "Run code"}
              </Button>
            )}
          </div>
        </div>
        <div className="workbench-body">
          <div className="editor-pane">
            <div className="pane-topline">
              <span>
                <span className="r-file">R</span> experiment.R
              </span>
              <div className="editor-header-actions">
                <Select
                  value={input.exampleId ?? "custom"}
                  onValueChange={(value) => {
                    const example = examples.find((x) => x.id === value)
                    if (example) onSelect(example)
                  }}
                >
                  <SelectTrigger
                    className="recipe-select"
                    aria-label="Choose an R example"
                  >
                    <SelectValue>
                      {examples.find((x) => x.id === input.exampleId)?.title ??
                        "Custom code"}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent
                    className="recipe-options"
                    align="end"
                    alignItemWithTrigger={false}
                  >
                    {examples.map((example) => (
                      <SelectItem key={example.id} value={example.id}>
                        {example.title}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="editor-surface">
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <Button
                        variant="outline"
                        className="icon-control editor-copy"
                        aria-label="Copy R code"
                        onClick={copy}
                      >
                        <span
                          className="copy-icon-swap"
                          data-copied={copied}
                          aria-hidden="true"
                        >
                          <Check className="copy-check" />
                          <Copy className="copy-original" />
                        </span>
                      </Button>
                    }
                  />
                  <TooltipContent>Copy</TooltipContent>
                </Tooltip>
              </TooltipProvider>
              {editorReady ? (
                <Suspense
                  fallback={
                    <textarea
                      aria-label="R code editor"
                      className="editor-fallback"
                      value={code}
                      onChange={(e) => changeCode(e.target.value)}
                    />
                  }
                >
                  <CodeEditor code={code} onChange={changeCode} onRun={run} />
                </Suspense>
              ) : (
                <textarea
                  className="editor-fallback"
                  aria-label="R code editor"
                  value={code}
                  onChange={(e) => changeCode(e.target.value)}
                />
              )}
            </div>
            <div className="editor-footer">
              <span>R · UTF-8</span>
              {!automatic && <span>⌘ / Ctrl + Enter to run</span>}
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
                  onClick={() => changeMode("plot")}
                >
                  <ChartNoAxesCombined size={14} />
                  Plot
                </button>
                <button
                  aria-pressed={mode === "console"}
                  onClick={() => changeMode("console")}
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
                ) : (
                  <div className="empty-result">
                    <ChartNoAxesCombined />
                    <p>Your plot will appear here.</p>
                    <span>
                      {automatic
                        ? "Examples run automatically."
                        : "Choose Run code to see the result."}
                    </span>
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
        <div className="sr-only">
          <span role="status">{feedback}</span>
        </div>
      </div>
      <details id="compatibility" className="compat-note">
        <summary>What can I run?</summary>
        <p>
          The examples here run in the browser: data frames, linear algebra,
          LOESS, common plots, and a subset of grid and mathematical labels.
        </p>
        <p>
          This is a partial R runtime. Arbitrary CRAN packages and native
          extensions are not supported. Some browser functions, including FFT,
          are still unavailable. Grid layouts and mathematical typography do not
          yet fully match GNU R.
        </p>
      </details>
    </section>
  )
}

function subscribeClient() {
  return () => {}
}
