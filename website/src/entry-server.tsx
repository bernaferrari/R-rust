import { StrictMode } from "react"
import { renderToString } from "react-dom/server"
import App from "./App"

export { pages } from "./pages"
import type { Page } from "./pages"
export function render(page: Page | "missing" = "home") {
  return renderToString(
    <StrictMode>
      <App page={page} />
    </StrictMode>
  )
}
