import { useVirtualizer } from "@tanstack/react-virtual"
import {
  consoleExamples,
  type ConsoleCommand,
  type ConsoleExample,
} from "@/console-examples"
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
import {
  Message,
  MessageContent,
  MessageHeader,
  MessageFooter,
} from "@/components/ui/message"
import { Bubble, BubbleContent } from "@/components/ui/bubble"
import {
  Select,
  SelectTrigger,
  SelectValue,
  SelectContent,
  SelectItem,
} from "@/components/ui/select"
import { ConsoleFiles } from "@/components/ConsoleFiles"
import { RRuntime } from "@/runtime/r-runtime"

type Entry = {
  id: number
  code: string
  output?: string
  image?: string
  error?: string
  ms?: number
}
export function RConsole() {
  const runtime = useRef<RRuntime | null>(null)
  const urls = useRef(new Set<string>())
  const serial = useRef(0)
  const generation = useRef(0)
  const locked = useRef(false)
  const input = useRef<HTMLTextAreaElement>(null)
  const [entries, setEntries] = useState<Entry[]>([])
  const [draft, setDraft] = useState("")
  const historyCursor = useRef<number | null>(null)
  const savedDraft = useRef("")
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState("")
  const viewport = useRef<HTMLDivElement>(null)
  const followOutput = useRef(true)
  // TanStack owns row measurement; Message Scroller owns the surrounding viewport controls.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => viewport.current,
    estimateSize: () => 260,
    overscan: 4,
    getItemKey: (index) => entries[index].id,
  })

  useEffect(() => {
    if (!entries.length || !followOutput.current) return
    const frame = requestAnimationFrame(() =>
      virtualizer.scrollToIndex(entries.length - 1, { align: "end" })
    )
    return () => cancelAnimationFrame(frame)
  }, [entries, virtualizer])
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
  function restore(code: string) {
    historyCursor.current = null
    setDraft(code)
    input.current?.focus()
  }
  function reset() {
    historyCursor.current = null
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
  async function execute(commands: ConsoleCommand[], next?: ConsoleCommand) {
    historyCursor.current = null
    if (locked.current || !runtime.current) return
    locked.current = true
    followOutput.current = true
    const epoch = generation.current
    setDraft("")
    setBusy(true)
    let currentId: number | undefined
    try {
      for (const command of commands) {
        if (generation.current !== epoch) return
        const item: Entry = { id: ++serial.current, ...command }
        currentId = item.id
        setEntries((previous) => [...previous.slice(-499), item])
        setNotice(
          next ? "Running the example in your R session…" : "R is working…"
        )
        const result = await runtime.current.run(item.code, "interactive")
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
              ? {
                  ...entry,
                  output: result.output,
                  error: result.error,
                  image,
                  ms: result.durationMs,
                }
              : entry
          )
        )
      }
      if (next) {
        setDraft(next.code)
      }
      setNotice("")
    } catch (error) {
      if (generation.current !== epoch) return
      const message = error instanceof Error ? error.message : String(error)
      setEntries((previous) =>
        previous.map((entry) =>
          entry.id === currentId ? { ...entry, error: message } : entry
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
  function run() {
    if (draft.trim()) return execute([{ code: draft }])
  }
  function openExample(example: ConsoleExample) {
    if (locked.current) return
    reset()
    void execute(example.commands, example.next)
  }
  useEffect(() => {
    const retained = new Set(
      entries.flatMap((entry) => (entry.image ? [entry.image] : []))
    )
    for (const url of urls.current) {
      if (!retained.has(url)) {
        URL.revokeObjectURL(url)
        urls.current.delete(url)
      }
    }
  }, [entries])
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
      </div>
      {entries.length > 0 && (
        <div className="r-chat-example-picker">
          <Select
            value={null}
            disabled={busy}
            onValueChange={(value) => {
              const example = consoleExamples.find(
                (item) => item.title === value
              )
              if (example) openExample(example)
            }}
          >
            <SelectTrigger aria-label="Open an example conversation">
              <SelectValue placeholder="Open an example conversation" />
            </SelectTrigger>
            <SelectContent>
              {consoleExamples.map((example) => (
                <SelectItem value={example.title} key={example.title}>
                  {example.title}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            onClick={reset}
            title="Clear history and all R variables"
          >
            <RotateCcw />
            New session
          </Button>
        </div>
      )}
      <ConsoleFiles runtime={runtime} busy={busy} revision={entries.length} />
      <div className="r-chat-workspace">
        <div className="r-chat-topline">
          <span>
            <Terminal size={16} /> R / WebAssembly
          </span>
          <span aria-label="History count">
            {entries.length ? `${entries.length} / 500 commands` : ""}
          </span>
        </div>
        <div className="r-chat-history">
          <MessageScrollerProvider>
            <MessageScroller>
              <MessageScrollerViewport
                ref={viewport}
                onScroll={(event) => {
                  const node = event.currentTarget
                  followOutput.current =
                    node.scrollHeight - node.scrollTop - node.clientHeight < 80
                }}
                aria-label="R command history"
              >
                <MessageScrollerContent
                  className={
                    entries.length ? "r-chat-virtual-content" : undefined
                  }
                  aria-busy={busy}
                >
                  {entries.length === 0 && (
                    <MessageScrollerItem
                      messageId="welcome"
                      className="r-chat-welcome"
                    >
                      <span className="r-chat-mark">R</span>
                      <h2>What are you curious about?</h2>
                      <p>
                        Write R below, or open a conversation already in motion.
                        <br />
                        Examples run real R, then leave the next move to you.
                      </p>
                      <div className="r-chat-starters">
                        {consoleExamples.map((example) => (
                          <Button
                            variant="outline"
                            key={example.title}
                            onClick={() => openExample(example)}
                          >
                            <span>
                              <strong>{example.title}</strong>
                              <small>{example.description}</small>
                            </span>
                            <CornerUpLeft size={14} />
                          </Button>
                        ))}
                      </div>
                    </MessageScrollerItem>
                  )}
                  {entries.length > 0 && (
                    <div
                      style={{
                        height: virtualizer.getTotalSize(),
                        position: "relative",
                        width: "100%",
                      }}
                    >
                      {virtualizer.getVirtualItems().map((virtualItem) => {
                        const entry = entries[virtualItem.index]
                        return (
                          <div
                            data-index={virtualItem.index}
                            ref={virtualizer.measureElement}
                            className="r-chat-turn"
                            key={entry.id}
                            style={{
                              position: "absolute",
                              top: 0,
                              left: 0,
                              width: "100%",
                              transform: `translateY(${virtualItem.start}px)`,
                            }}
                          >
                            <Message align="end">
                              <MessageContent>
                                <MessageHeader className="sr-only">
                                  <span className="sr-only">
                                    You · R command
                                  </span>
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
                                  onClick={() => restore(entry.code)}
                                >
                                  <CornerUpLeft />
                                  Reuse
                                </Button>
                              </MessageContent>
                            </Message>
                            <Message>
                              <MessageContent>
                                <span className="sr-only">R response</span>
                                <Bubble
                                  variant={
                                    entry.error ? "destructive" : "muted"
                                  }
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
                                      <pre>
                                        {entry.output
                                          ? `${entry.output.trim()}\n${entry.error}`
                                          : entry.error}
                                      </pre>
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
                                {entry.ms !== undefined && (
                                  <MessageFooter>
                                    <span>{Math.round(entry.ms)} ms</span>
                                  </MessageFooter>
                                )}
                              </MessageContent>
                            </Message>
                          </div>
                        )
                      })}
                    </div>
                  )}
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
            onChange={(event) => {
              historyCursor.current = null
              setDraft(event.target.value)
            }}
            placeholder={"Try mean(c(2, 4, 8))"}
            onKeyDown={(event) => {
              const field = event.currentTarget
              if (
                !event.nativeEvent.isComposing &&
                !event.shiftKey &&
                !event.altKey &&
                !event.ctrlKey &&
                !event.metaKey &&
                field.selectionStart === field.selectionEnd
              ) {
                const atFirstLine = !draft
                  .slice(0, field.selectionStart)
                  .includes("\n")
                const atLastLine = !draft
                  .slice(field.selectionEnd)
                  .includes("\n")
                if (event.key === "ArrowUp" && atFirstLine && entries.length) {
                  event.preventDefault()
                  if (historyCursor.current === null) savedDraft.current = draft
                  const index = Math.max(
                    0,
                    (historyCursor.current ?? entries.length) - 1
                  )
                  historyCursor.current = index
                  setDraft(entries[index].code)
                  requestAnimationFrame(() => field.setSelectionRange(0, 0))
                  return
                }
                if (
                  event.key === "ArrowDown" &&
                  atLastLine &&
                  historyCursor.current !== null
                ) {
                  event.preventDefault()
                  const index = historyCursor.current + 1
                  historyCursor.current = index < entries.length ? index : null
                  const value =
                    index < entries.length
                      ? entries[index].code
                      : savedDraft.current
                  setDraft(value)
                  requestAnimationFrame(() =>
                    field.setSelectionRange(value.length, value.length)
                  )
                  return
                }
              }
              if (
                event.key === "Enter" &&
                !event.shiftKey &&
                !event.nativeEvent.isComposing
              ) {
                event.preventDefault()
                historyCursor.current = null
                void run()
              }
            }}
          />
          <div className="r-chat-compose-actions">
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
                size="icon"
              >
                <ArrowUp />
              </Button>
            )}
          </div>
          {notice && (
            <div className="r-chat-status" role="status">
              {notice}
            </div>
          )}
        </form>
      </div>
      <p className="r-chat-footnote">
        A browser R runtime, still evolving.{" "}
        <a href="../compatibility/">Compatibility & limits ↗</a>
      </p>
    </section>
  )
}
