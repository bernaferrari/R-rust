import { readFile, writeFile, mkdir } from "node:fs/promises"
import { resolve, dirname } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
const root = resolve(fileURLToPath(new URL("..", import.meta.url)))
const template = await readFile(resolve(root, "dist/index.html"), "utf8")
const { render, pages } = await import(pathToFileURL(resolve(root, "dist-server/entry-server.js")).href)
const escape = value => value.replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
// Supply the complete deployment URL, including a subdirectory if applicable.
const site = process.env.SITE_URL ? new URL(process.env.SITE_URL.endsWith("/") ? process.env.SITE_URL : process.env.SITE_URL + "/") : null
if (site && !["https:", "http:"].includes(site.protocol)) throw new Error("SITE_URL must be an HTTP(S) URL")
for (const [key, page] of Object.entries(pages)) {
  const canonical = site ? new URL(page.path, site).href : null
  let html = template.replace('<div id="root"></div>', `<div id="root">${render(key)}</div>`)
    .replace(/<title>.*?<\/title>/s, `<title>${escape(page.title)}</title>`)
    .replace(/<meta\s+name="description"\s+content="[^"]*"\s*\/>/s, `<meta name="description" content="${escape(page.description)}" />`)
    .replace(/<meta\s+property="og:title"\s+content="[^"]*"\s*\/>/s, `<meta property="og:title" content="${escape(page.title)}" />`)
    .replace(/<meta\s+property="og:description"\s+content="[^"]*"\s*\/>/s, `<meta property="og:description" content="${escape(page.description)}" />`)
  const metadata = ['<meta name="twitter:card" content="summary_large_image" />']
  if (canonical) metadata.push(`<link rel="canonical" href="${escape(canonical)}" />`, `<meta property="og:url" content="${escape(canonical)}" />`, `<meta property="og:image" content="${escape(new URL("examples/loess.png", site).href)}" />`)
  html = html.replace("</head>", metadata.join("\n") + "\n</head>")
  const dest = resolve(root, "dist", page.path, "index.html")
  await mkdir(dirname(dest), { recursive: true })
  await writeFile(dest, html)
}
await writeFile(resolve(root, "dist/404.html"), template.replace('<div id="root"></div>', `<div id="root">${render("missing")}</div>`).replace(/<title>.*?<\/title>/s, '<title>Page not found | Rove</title>').replace('</head>', '<meta name="robots" content="noindex" /></head>'))
if (site) {
  await writeFile(resolve(root, "dist/sitemap.xml"), `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${Object.values(pages).map(page => `<url><loc>${escape(new URL(page.path, site).href)}</loc></url>`).join("\n")}</urlset>`)
}
await writeFile(resolve(root, "dist/robots.txt"), `User-agent: *\nAllow: /\n${site ? `Sitemap: ${new URL("sitemap.xml", site).href}\n` : ""}`)
console.log(`Prerendered ${Object.keys(pages).length} pages and a 404 page.${site ? " Canonical URLs and sitemap generated." : " Set SITE_URL to generate canonical URLs and sitemap.xml for deployment."}`)
