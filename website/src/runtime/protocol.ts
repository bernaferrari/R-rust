export type RuntimeMode = "plot" | "console" | "interactive"

export type RuntimeCommand =
  | { code: string; mode: RuntimeMode; action?: "run" }
  | { action: "import-file"; path: string; bytes: Uint8Array }
  | { action: "export-file" | "remove-file"; path: string }
  | { action: "list-files" }

export type RuntimeRequest = RuntimeCommand & { id: number }

export type RuntimeResponse =
  | {
      id: number
      ok: true
      output: string
      png?: Uint8Array
      files?: string[]
      file?: Uint8Array
      error?: string
      durationMs: number
    }
  | { id: number; ok: false; error: string; fatal?: boolean }

export type RuntimeStatus =
  "idle" | "loading" | "running" | "ready" | "error" | "reset"
