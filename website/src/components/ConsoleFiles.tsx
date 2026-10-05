import { useEffect, useRef, useState, type RefObject } from "react"
import { Download, FolderOpen, Upload } from "lucide-react"
import { Button } from "@/components/ui/button"
import type { RRuntime } from "@/runtime/r-runtime"

/** Files belong to the R session, not to the host computer's filesystem. */
export function ConsoleFiles({
  runtime,
  busy,
  revision,
}: {
  runtime: RefObject<RRuntime | null>
  busy: boolean
  revision: string
}) {
  const input = useRef<HTMLInputElement>(null)
  const [open, setOpen] = useState(false)
  const [files, setFiles] = useState<string[]>([])
  const [error, setError] = useState("")
  const [working, setWorking] = useState(false)

  useEffect(() => {
    if (!open || busy) return
    let current = true
    runtime.current?.listFiles().then(
      (names) => {
        if (current) setFiles(names)
      },
      (cause) => {
        if (current) setError(String(cause))
      }
    )
    return () => {
      current = false
    }
  }, [open, busy, revision, runtime])

  async function upload(file: File) {
    const session = runtime.current
    if (!session) return
    const generation = session.sessionGeneration
    const isCurrent = () =>
      runtime.current === session && session.sessionGeneration === generation
    setWorking(true)
    setError("")
    try {
      if (file.size > 1024 * 1024)
        throw new Error("Choose a file smaller than 1 MiB.")
      const names = await session.listFiles()
      if (names.includes(file.name))
        throw new Error(
          "A file with that name is already in this session. Rename it before importing."
        )
      const bytes = new Uint8Array(await file.arrayBuffer())
      if (!isCurrent()) return
      await session.importFile(file.name, bytes)
      if (!isCurrent()) return
      const imported = await session.listFiles()
      if (isCurrent()) setFiles(imported)
    } catch (cause) {
      if (isCurrent())
        setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setWorking(false)
    }
  }

  async function download(name: string) {
    const session = runtime.current
    if (!session) return
    setWorking(true)
    setError("")
    try {
      const bytes = await session.exportFile(name)
      const url = URL.createObjectURL(
        new Blob([new Uint8Array(bytes)], { type: "application/octet-stream" })
      )
      const link = document.createElement("a")
      link.href = url
      link.download = name.split("/").at(-1) || name
      link.click()
      setTimeout(() => URL.revokeObjectURL(url), 1000)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setWorking(false)
    }
  }

  return (
    <details
      className="r-chat-files"
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>
        <FolderOpen size={16} aria-hidden="true" /> Session files
      </summary>
      <div className="r-chat-files-panel">
        <p>
          Import a file, then use its name in R—for example,{" "}
          <code>read.csv("data.csv")</code> or <code>source("script.R")</code>.
          Files stay here until the session ends.
        </p>
        <input
          ref={input}
          type="file"
          className="sr-only"
          aria-label="Import a session file"
          disabled={busy || working}
          onChange={(event) => {
            const file = event.target.files?.[0]
            event.target.value = ""
            if (file) void upload(file)
          }}
        />
        <Button
          size="sm"
          variant="outline"
          disabled={busy || working}
          onClick={() => input.current?.click()}
        >
          <Upload /> Import file
        </Button>
        {files.length > 0 && (
          <ul>
            {files.map((name) => (
              <li key={name}>
                <code>{name}</code>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  aria-label={`Download ${name}`}
                  disabled={busy || working}
                  onClick={() => void download(name)}
                >
                  <Download />
                </Button>
              </li>
            ))}
          </ul>
        )}
        {error && <p role="alert">{error}</p>}
      </div>
    </details>
  )
}
