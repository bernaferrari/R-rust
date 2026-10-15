import config from "./playwright.config"
import { defineConfig } from "@playwright/test"
export default defineConfig({
  ...config,
  use: { ...config.use, baseURL: "http://127.0.0.1:5175" },
  webServer: {
    command: "pnpm dev --host 127.0.0.1 --port 5175 --strictPort",
    url: "http://127.0.0.1:5175",
    reuseExistingServer: false,
    timeout: 120_000,
  },
})
