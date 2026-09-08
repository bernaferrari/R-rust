import { useSyncExternalStore } from "react"

function subscribe(onChange: () => void) {
  const media = window.matchMedia("(prefers-color-scheme: dark)")
  const sync = () => {
    let preference: string | null = null
    try {
      preference = localStorage.getItem("rove-theme")
    } catch {
      /* Storage may be unavailable. */
    }
    applyTheme(
      preference === "dark" || (preference !== "light" && media.matches)
    )
    onChange()
  }
  media.addEventListener("change", sync)
  window.addEventListener("storage", sync)
  window.addEventListener("rove-theme-change", onChange)
  return () => {
    media.removeEventListener("change", sync)
    window.removeEventListener("storage", sync)
    window.removeEventListener("rove-theme-change", onChange)
  }
}
function applyTheme(dark: boolean) {
  if (document.documentElement.classList.contains("dark") !== dark) {
    document.documentElement.classList.add("theme-changing")
    requestAnimationFrame(() =>
      requestAnimationFrame(() =>
        document.documentElement.classList.remove("theme-changing")
      )
    )
  }
  document.documentElement.classList.toggle("dark", dark)
  document.documentElement.style.colorScheme = dark ? "dark" : "light"
  document
    .querySelector('meta[name="theme-color"]')
    ?.setAttribute("content", dark ? "#171e1c" : "#f8f6ef")
}
export function useDarkTheme() {
  return useSyncExternalStore(
    subscribe,
    () => document.documentElement.classList.contains("dark"),
    () => false
  )
}
export function toggleTheme() {
  const dark = !document.documentElement.classList.contains("dark")
  try {
    localStorage.setItem("rove-theme", dark ? "dark" : "light")
  } catch {
    /* The choice still applies for this visit. */
  }
  applyTheme(dark)
  window.dispatchEvent(new Event("rove-theme-change"))
}
