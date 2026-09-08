export const pages = {
  home: {
    path: "",
    label: "Home",
    title: "Rove — R belongs everywhere.",
    description:
      "Explore R in your browser: runnable examples, Vello graphics and local AI, powered by a Rust and WebAssembly runtime.",
  },
  editor: {
    path: "editor/",
    label: "R editor",
    title: "Online R editor — Run R in your browser | Rove",
    description:
      "Write and run R in a focused browser editor. Explore data, render plots, and choose automatic or manual execution with WebAssembly.",
  },
  examples: {
    path: "examples/",
    label: "Examples",
    title:
      "16 runnable R examples — Statistics, graphics and creative coding | Rove",
    description:
      "Explore editable R examples: LOESS curves, distributions, grid calendars, mathematical labels, sunflowers and everyday data analysis.",
  },
  ai: {
    path: "local-ai/",
    label: "Local AI",
    title: "Local AI for R — Browser models and Ollama | Rove",
    description:
      "Draft R code with a local language model, review it, and run the analysis. Use a WebGPU browser model or connect Ollama.",
  },
  embed: {
    path: "embedding/",
    label: "For builders",
    title: "Embed R in Rust, web and mobile apps | Rove",
    description:
      "Explore Rove's Rust embedding API, WebAssembly worker runtime, Kotlin and Swift bindings, and portable Vello graphics.",
  },
  compatibility: {
    path: "compatibility/",
    label: "Compatibility",
    title: "R compatibility and runtime limits | Rove",
    description:
      "What Rove supports today, what differs from GNU R, and the current package, graphics and runtime safety limits.",
  },
} as const
export type Page = keyof typeof pages
export function pageHref(page: Page) {
  return import.meta.env.BASE_URL + pages[page].path
}
export function resolvePage(pathname: string): Page | "missing" {
  const base = import.meta.env.BASE_URL
  const path = pathname.startsWith(base)
    ? pathname.slice(base.length)
    : pathname.replace(/^\//, "")
  const normalized = path.replace(/index\.html$/, "").replace(/\/?$/, "/")
  return (
    (Object.keys(pages) as Page[]).find(
      (key) => (pages[key].path || "/") === normalized
    ) ?? "missing"
  )
}
