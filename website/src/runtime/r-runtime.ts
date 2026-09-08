import type {
  RuntimeMode,
  RuntimeResponse,
  RuntimeStatus,
  RuntimeCommand,
} from "./protocol"

export type RuntimeResult = {
  output: string
  png?: Uint8Array
  files?: string[]
  file?: Uint8Array
  error?: string
  durationMs: number
}

export type RRuntimeOptions = {
  timeoutMs?: number
  onStatus?: (status: RuntimeStatus) => void
}

type Pending = {
  resolve: (result: RuntimeResult) => void
  reject: (error: Error) => void
  timer: ReturnType<typeof setTimeout>
}

const DEFAULT_TIMEOUT_MS = 15_000
const MAX_PENDING = 16

/** A lazily initialized, isolated R interpreter running in a Web Worker. */
export class RRuntime {
  private worker?: Worker
  private nextId = 0
  private generation = 0
  private pending = new Map<number, Pending>()
  private readonly timeoutMs: number
  private readonly onStatus?: (status: RuntimeStatus) => void

  constructor(options: RRuntimeOptions = {}) {
    this.timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS
    this.onStatus = options.onStatus
  }

  run(code: string, mode: RuntimeMode): Promise<RuntimeResult> {
    if (typeof code !== "string" || code.length === 0)
      return Promise.reject(new Error("Runtime code cannot be empty"))
    return this.request({ code, mode })
  }
  async importFile(path: string, bytes: Uint8Array): Promise<void> {
    if (bytes.byteLength > 1024 * 1024)
      throw new Error("Files are limited to 1 MiB each")
    await this.request({ action: "import-file", path, bytes })
  }
  async exportFile(path: string): Promise<Uint8Array> {
    const result = await this.request({ action: "export-file", path })
    if (!result.file) throw new Error("No file returned by the R session")
    return result.file
  }
  async listFiles(): Promise<string[]> {
    return (await this.request({ action: "list-files" })).files ?? []
  }
  async removeFile(path: string): Promise<void> {
    await this.request({ action: "remove-file", path })
  }
  private request(command: RuntimeCommand): Promise<RuntimeResult> {
    if (this.pending.size >= MAX_PENDING)
      return Promise.reject(
        new Error(
          "Too many runtime requests; wait for the current work to finish"
        )
      )
    this.ensureWorker()
    const id = ++this.nextId
    const generation = this.generation
    this.onStatus?.("running")
    return new Promise<RuntimeResult>((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!this.pending.delete(id)) return
        this.resetWorker(new Error("R runtime timed out and was reset"))
        reject(new Error("R runtime timed out and was reset"))
      }, this.timeoutMs)
      this.pending.set(id, { resolve, reject, timer })
      if (generation !== this.generation) return
      this.worker!.postMessage({ id, ...command })
    })
  }

  reset(): void {
    this.resetWorker(
      new Error("R runtime reset; in-memory objects were cleared")
    )
  }

  dispose(): void {
    this.resetWorker(new Error("R runtime disposed"))
    this.onStatus?.("reset")
  }

  private ensureWorker() {
    if (this.worker) return
    this.onStatus?.("loading")
    const worker = new Worker(
      new URL("./r-runtime-worker.ts", import.meta.url),
      { type: "module" }
    )
    worker.onmessage = ({ data }: MessageEvent<RuntimeResponse>) =>
      this.receive(data)
    worker.onerror = () =>
      this.resetWorker(new Error("R runtime worker failed and was reset"))
    this.worker = worker
  }

  private receive(data: RuntimeResponse) {
    const request = this.pending.get(data.id)
    if (!request) return // stale response from a terminated worker
    clearTimeout(request.timer)
    this.pending.delete(data.id)
    if (!data.ok) {
      request.reject(new Error(data.error))
      if (data.fatal)
        this.resetWorker(new Error(`${data.error}; session reset`))
      else this.onStatus?.("error")
      return
    }
    request.resolve({
      output: data.output,
      png: data.png,
      files: data.files,
      file: data.file,
      error: data.error,
      durationMs: data.durationMs,
    })
    this.onStatus?.("ready")
  }

  private resetWorker(reason: Error) {
    this.generation += 1
    this.worker?.terminate()
    this.worker = undefined
    for (const request of this.pending.values()) {
      clearTimeout(request.timer)
      request.reject(reason)
    }
    this.pending.clear()
    this.onStatus?.("reset")
  }
}

export type { RuntimeMode, RuntimeStatus } from "./protocol"
