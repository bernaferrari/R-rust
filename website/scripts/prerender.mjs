import { readFile, writeFile } from "node:fs/promises"
import { resolve } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

const root = resolve(fileURLToPath(new URL("..", import.meta.url)))
const templatePath = resolve(root, "dist/index.html")
const serverPath = resolve(root, "dist-server/entry-server.js")
const template = await readFile(templatePath, "utf8")
const { render } = await import(pathToFileURL(serverPath).href)
const html = template.replace(
  '<div id="root"></div>',
  `<div id="root">${render()}</div>`
)
await writeFile(templatePath, html)
console.log(`Prerendered ${templatePath}`)
