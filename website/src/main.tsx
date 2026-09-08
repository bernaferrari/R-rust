import { StrictMode } from "react"
import { createRoot, hydrateRoot } from "react-dom/client"
import "./index.css"
import App from "./App"
import { resolvePage, pages } from "./pages"
const page = resolvePage(window.location.pathname)
if (page !== "missing") document.title = pages[page].title
const root = document.getElementById("root")!
const app = (
  <StrictMode>
    <App page={page} />
  </StrictMode>
)
if (root.hasChildNodes()) hydrateRoot(root, app)
else createRoot(root).render(app)
