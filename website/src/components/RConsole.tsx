import {
  MessageScroller,
  MessageScrollerProvider,
  MessageScrollerViewport,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerButton,
} from "@/components/ui/message-scroller"
import { useEffect, useRef, useState } from "react"
import {
  ArrowUp,
  RotateCcw,
  Square,
  Terminal,
  CornerUpLeft,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { Message, MessageContent, MessageHeader } from "@/components/ui/message"
import { Bubble, BubbleContent } from "@/components/ui/bubble"
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from "@/components/ui/select"
import { RRuntime } from "@/runtime/r-runtime"
import type { RuntimeMode } from "@/runtime/protocol"

type Entry = {
  id: number
  code: string
  mode: RuntimeMode
  output?: string
  image?: string
  error?: string
  ms?: number
}
const starters = [
  {
    title: "Start with a little data",
    code: "temperatures <- c(19, 22, 24, 21, 18)\nmean(temperatures)",
    mode: "console" as const,
  },
  {
    title: "Draw a curve",
    code: 'x <- seq(0, 2 * pi, length.out = 120)\nplot(x, sin(x), type = "l", lwd = 3, col = "#16766c",\n     main = "A little rhythm", xlab = "Time", ylab = "Signal")',
    mode: "plot" as const,
  },
  {
    title: "Ask R a question",
    code: "summary(cars)",
    mode: "console" as const,
  },
]
export function RConsole() {
  const runtime = useRef<RRuntime | null>(null)
  const urls = useRef(new Set<string>())
  const serial = useRef(0)
  const generation = useRef(0)
  const locked = useRef(false)
  const input = useRef<HTMLTextAreaElement>(null)
  const [entries, setEntries] = useState<Entry[]>([])
  const [draft, setDraft] = useState("")
  const [mode, setMode] = useState<RuntimeMode>("console")
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState("Ready when you are")
  useEffect(() => {
    runtime.current = new RRuntime()
    const ownedUrls = urls.current
    return () => {
      // This counter invalidates pending promises when the session unmounts.
      // eslint-disable-next-line react-hooks/exhaustive-deps
      generation.current++
      runtime.current?.dispose()
      ownedUrls.forEach(URL.revokeObjectURL)
    }
  }, [])
  function restore(code: string, nextMode: RuntimeMode) {
    setDraft(code)
    setMode(nextMode)
    input.current?.focus()
  }
  function reset() {
    generation.current++
    runtime.current?.reset()
    locked.current = false
    setBusy(false)
    urls.current.forEach(URL.revokeObjectURL)
    urls.current.clear()
    setEntries([])
    setNotice("Fresh session. Previous variables were cleared.")
    input.current?.focus()
  }
  async function run() {
    if (locked.current || !draft.trim() || !runtime.current) return
    locked.current = true
    const epoch = generation.current
    const item: Entry = { id: ++serial.current, code: draft, mode }
    setEntries((previous) => {
      const removed = previous.length >= 50 ? previous[0] : undefined
      if (removed?.image) {
        URL.revokeObjectURL(removed.image)
        urls.current.delete(removed.image)
      }
      return [...previous.slice(-49), item]
    })
    setDraft("")
    setBusy(true)
    setNotice("R is working…")
    try {
      const result = await runtime.current.run(item.code, item.mode)
      if (generation.current !== epoch) return
      const image = result.png
        ? URL.createObjectURL(
            new Blob([new Uint8Array(result.png)], { type: "image/png" })
          )
        : undefined
      if (image) urls.current.add(image)
      setEntries((previous) =>
        previous.map((entry) =>
          entry.id === item.id
            ? { ...entry, output: result.output, image, ms: result.durationMs }
            : entry
        )
      )
      setNotice("Ready · variables retained in this session")
    } catch (error) {
      if (generation.current !== epoch) return
      const message = error instanceof Error ? error.message : String(error)
      setEntries((previous) =>
        previous.map((entry) =>
          entry.id === item.id ? { ...entry, error: message } : entry
        )
      )
      setNotice(
        /reset|panic|closed|out of memory/i.test(message)
          ? "Session reset. Run your setup commands again."
          : "Ready · correct the command and try again"
      )
    } finally {
      if (generation.current === epoch) {
        locked.current = false
        setBusy(false)
        input.current?.focus()
      }
    }
  }
  return (
    <section className="r-chat" aria-label="Interactive R console">
      <div className="r-chat-heading">
        <div>
          <span className="eyebrow">THE R CONSOLE</span>
          <h1>
            A conversation <em>with R.</em>
          </h1>
          <p>
            A command, a discovery, another question. Pick up where you left
            off.
          </p>
        </div>
        <Button
          variant="outline"
          onClick={reset}
          title="Clear history and all R variables"
        >
          <RotateCcw />
          New session
        </Button>
      </div>
      <div className="r-chat-workspace">
        <div className="r-chat-topline">
          <span>
            <Terminal size={16} /> R / WebAssembly
          </span>
          <span>Session lasts until you leave this page</span>
        </div>
        <div className="r-chat-history">
          <MessageScrollerProvider>
            <MessageScroller>
              <MessageScrollerViewport aria-label="R command history">
                <MessageScrollerContent aria-busy={busy}>
                  {entries.length === 0 && (
                    <MessageScrollerItem
                      messageId="welcome"
                      className="r-chat-welcome"
                    >
                      <span className="r-chat-mark">R</span>
                      <h2>What are you curious about?</h2>
                      <p>
                        Write R below, or start with a small experiment.
                        <br />
                        Your variables are available to the next command.
                      </p>
                      <div className="r-chat-starters">
                        {starters.map((starter) => (
                          <Button
                            variant="outline"
                            key={starter.title}
                            onClick={() => restore(starter.code, starter.mode)}
                          >
                            {starter.title}
                            <CornerUpLeft size={14} />
                          </Button>
                        ))}
                      </div>
                    </MessageScrollerItem>
                  )}
                  {entries.map((entry) => (
                    <MessageScrollerItem
                      messageId={String(entry.id)}
                      scrollAnchor
                      className="r-chat-turn"
                      key={entry.id}
                    >
                      <Message align="end">
                        <MessageContent>
                          <MessageHeader>
                            You · {entry.mode === "plot" ? "plot" : "R command"}
                          </MessageHeader>
                          <Bubble variant="tinted">
                            <BubbleContent>
                              <pre>{entry.code}</pre>
                            </BubbleContent>
                          </Bubble>
                          <Button
                            variant="ghost"
                            size="sm"
                            aria-label={`Reuse command ${entry.id}`}
                            onClick={() => restore(entry.code, entry.mode)}
                          >
                            <CornerUpLeft />
                            Reuse
                          </Button>
                        </MessageContent>
                      </Message>
                      <Message>
                        <MessageContent>
                          <MessageHeader>
                            R{" "}
                            {entry.ms !== undefined && (
                              <span> · {Math.round(entry.ms)} ms</span>
                            )}
                          </MessageHeader>
                          <Bubble
                            variant={entry.error ? "destructive" : "ghost"}
                          >
                            <BubbleContent>
                              {entry.image && (
                                <a
                                  href={entry.image}
                                  download={`r-plot-${entry.id}.png`}
                                  title="Download plot"
                                >
                                  <img
                                    src={entry.image}
                                    alt={`Plot from command ${entry.id}`}
                                  />
                                </a>
                              )}
                              {entry.error ? (
                                <pre>{entry.error}</pre>
                              ) : entry.ms !== undefined ? (
                                <pre>
                                  {(entry.output ?? "").trim() ||
                                    (entry.image ? "" : "Done.")}
                                </pre>
                              ) : (
                                <span role="status">Running…</span>
                              )}
                            </BubbleContent>
                          </Bubble>
                        </MessageContent>
                      </Message>
                    </MessageScrollerItem>
                  ))}
                </MessageScrollerContent>
              </MessageScrollerViewport>
              <MessageScrollerButton />
            </MessageScroller>
          </MessageScrollerProvider>
        </div>
        <form
          className="r-chat-composer"
          onSubmit={(event) => {
            event.preventDefault()
            void run()
          }}
        >
          <label className="sr-only" htmlFor="r-command">
            R command
          </label>
          <Textarea
            ref={input}
            id="r-command"
            value={draft}
            maxLength={65536}
            onChange={(event) => setDraft(event.target.value)}
            placeholder={"Try mean(c(2, 4, 8))"}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.nativeEvent.isComposing
              ) {
                event.preventDefault()
                void run()
              }
            }}
          />
          <div className="r-chat-compose-actions">
            <Select
              value={mode}
              onValueChange={(value) => {
                if (value === "console" || value === "plot") setMode(value)
              }}
            >
              <SelectTrigger aria-label="Command output">
                <SelectValue>
                  {mode === "console" ? "Text output" : "Plot output"}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="console">Text output</SelectItem>
                <SelectItem value="plot">Plot output</SelectItem>
              </SelectContent>
            </Select>
            <span>Shift + Enter for a new line</span>
            {busy ? (
              <Button type="button" variant="destructive" onClick={reset}>
                <Square />
                Stop & reset
              </Button>
            ) : (
              <Button
                type="submit"
                disabled={!draft.trim()}
                aria-label="Run command"
              >
                <ArrowUp />
                Run
              </Button>
            )}
          </div>
          <div className="r-chat-status" role="status">
            {notice}
          </div>
        </form>
      </div>
      <p className="r-chat-footnote">
        A browser R runtime, still evolving. Plot output creates a new image for
        each command. History keeps the latest 50 commands.{" "}
        <a href="../compatibility/">Compatibility & limits ↗</a>
      </p>
    </section>
  )
}
