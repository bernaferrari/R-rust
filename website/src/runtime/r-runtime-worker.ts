import init, { WasmRSession } from "./assets/r_wasm.js"
import type { RuntimeRequest, RuntimeResponse } from "./protocol"

const MAX_CODE_LENGTH = 64 * 1024
const WIDTH = 800
const HEIGHT = 600

let session: WasmRSession | undefined
let initPromise: Promise<WasmRSession> | undefined
let queue = Promise.resolve()

async function getSession(): Promise<WasmRSession> {
  if (!initPromise) {
    initPromise = init().then(() => new WasmRSession())
  }
  session = await initPromise
  return session
}

function post(message: RuntimeResponse, transfer?: Transferable[]) {
  self.postMessage(message, { transfer: transfer ?? [] })
}

self.onmessage = ({ data }: MessageEvent<RuntimeRequest>) => {
  const request = data
  queue = queue.then(async () => {
    const started = performance.now()
    try {
      if (!request) throw new Error("Missing runtime request")
      if (request.action && request.action !== "run") {
        if ("path" in request && typeof request.path !== "string")
          throw new Error("File path must be a string")
        const runtime = await getSession()
        let file: Uint8Array | undefined
        let files: string[] | undefined
        switch (request.action) {
          case "import-file":
            if (
              !(request.bytes instanceof Uint8Array) ||
              request.bytes.byteLength > 1024 * 1024
            )
              throw new Error("Files are limited to 1 MiB each")
            runtime.import_file(request.path, request.bytes)
            break
          case "export-file":
            file = runtime.export_file(request.path)
            break
          case "remove-file":
            runtime.remove_file(request.path)
            break
          case "list-files":
            files = runtime.list_files()
            break
          default:
            throw new Error("Unknown file operation")
        }
        post(
          {
            id: request.id,
            ok: true,
            output: "",
            durationMs: performance.now() - started,
            file,
            files,
          },
          file ? [file.buffer] : undefined
        )
        return
      }
      if (typeof request.code !== "string")
        throw new Error("Runtime code must be a string")
      if (request.code.length > MAX_CODE_LENGTH) {
        throw new Error(
          `Runtime code is limited to ${MAX_CODE_LENGTH.toLocaleString()} characters`
        )
      }
      const runtime = await getSession()
      let output = ""
      let png: Uint8Array | undefined
      let evaluationError: string | undefined
      if (request.mode === "interactive") {
        const result = runtime.eval_interactive(request.code, WIDTH, HEIGHT)
        try {
          output = result.output()
          if (result.has_png()) png = result.png()
          if (result.has_error()) evaluationError = result.error()
        } finally {
          result.free()
        }
      } else if (request.mode === "plot") {
        // render_png evaluates and captures the script atomically. Do not eval first.
        png = runtime.render_png(request.code, WIDTH, HEIGHT)
      } else if (request.mode === "console") {
        output = runtime.eval_checked(request.code)
      } else {
        throw new Error(`Unknown runtime mode: ${String(request.mode)}`)
      }
      post(
        {
          id: request.id,
          ok: true,
          output,
          ...(png ? { png } : {}),
          ...(evaluationError ? { error: evaluationError } : {}),
          durationMs: performance.now() - started,
        },
        png ? [png.buffer] : undefined
      )
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      const fatal =
        error instanceof WebAssembly.RuntimeError ||
        /panic|session closed|out of memory/i.test(message)
      post({ id: request?.id, ok: false, error: message, fatal })
    }
  })
}
