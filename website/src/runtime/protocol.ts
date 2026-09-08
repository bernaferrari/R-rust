export type RuntimeMode = "plot" | "console"

export type RuntimeRequest = {
  id: number
  code: string
  mode: RuntimeMode
}

export type RuntimeResponse =
  | {
      id: number
      ok: true
      output: string
      png?: Uint8Array
      durationMs: number
    }
  | { id: number; ok: false; error: string; fatal?: boolean }

export type RuntimeStatus =
  "idle" | "loading" | "running" | "ready" | "error" | "reset"
