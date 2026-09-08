import {
  NavigationMenu,
  NavigationMenuList,
  NavigationMenuItem,
  NavigationMenuLink,
  NavigationMenuTrigger,
  NavigationMenuContent,
} from "@/components/ui/navigation-menu"
import { RConsole } from "@/components/RConsole"
import { useState, useSyncExternalStore } from "react"
import {
  ArrowDown,
  ArrowUpRight,
  ArrowRight,
  Check,
  Copy,
  Code2,
  Compass,
  Moon,
  Sun,
  Globe2,
  Search,
  Smartphone,
  Sparkles,
  Terminal,
} from "lucide-react"
import { Toaster } from "@/components/ui/sonner"
import { Button } from "@/components/ui/button"
import { Playground, type PlaygroundInput } from "@/components/Playground"
import { LocalAI } from "@/components/LocalAI"
import {
  examples,
  categories,
  getExampleCode,
  type Example,
} from "@/data/examples"
import { toggleTheme, useDarkTheme } from "@/theme"
import { pages, pageHref, type Page } from "@/pages"
import { CompatibilityPage } from "@/components/CompatibilityPage"

function ThemeToggle() {
  const dark = useDarkTheme()
  return (
    <button
      className="theme-toggle"
      onClick={toggleTheme}
      aria-label={dark ? "Switch to light theme" : "Switch to dark theme"}
    >
      <Sun className="theme-sun" size={18} />
      <Moon className="theme-moon" size={18} />
    </button>
  )
}

const github = "https://github.com/bernaferrari/R-rust"
const codeSamples = {
  Browser: `import init, { WasmRSession } from "./r_wasm.js";

await init();
const r = new WasmRSession();
const result = r.eval_checked("mean(c(4, 8, 15, 16))");
// [1] 10.75
const png = r.render_png("plot(1:10)", 640, 480);
r.close();`,
  "Local AI": `// The model writes R. The runtime does the computation.
const draft = await model.chat.completions.create({
  messages: [{ role: "user", content: "Write R for a histogram" }],
});

// Review the draft, then run it in an isolated worker.
const result = await runtime.run(reviewedCode, "plot");
// Render result.png in your interface.`,
  Mobile: `// The same interpreter, embedded in your Rust host.
let mut r = r_embed::RSession::new()?;
let result = r.eval("mean(c(4, 8, 15, 16))")?;
let png = r.render_with_dimensions(
    "plot(1:10)", 640, 480,
)?;
// r-uniffi exposes session APIs to Kotlin and Swift.
// Your app owns the interface. R handles the analysis.`,
}
function Gallery({
  onSelect,
  standalone = false,
}: {
  onSelect: (example: Example) => void
  standalone?: boolean
}) {
  const CardHeading = standalone ? "h2" : "h3"
  const dark = useDarkTheme()
  const [category, setCategory] = useState("All examples"),
    [query, setQuery] = useState("")
  const visible = examples.filter(
    (e) =>
      (category === "All examples" || e.category === category) &&
      `${e.title} ${e.description} ${e.category}`
        .toLowerCase()
        .includes(query.toLowerCase())
  )
  return (
    <section id="examples" className="section examples-section">
      <div className="section-heading">
        <div>
          <span className="eyebrow">02 / FOLLOW A HUNCH</span>
          <h2>
            What will you <em>make?</em>
          </h2>
        </div>
        <p>
          {examples.length} ideas to start from.
          <br />
          Every example opens as editable, runnable R.
        </p>
      </div>
      <div className="gallery-controls">
        <div className="filter-tabs" role="group" aria-label="Filter examples">
          {categories.map((c) => (
            <button
              key={c}
              aria-pressed={category === c}
              onClick={() => setCategory(c)}
            >
              {c}
              {c === "All examples" ? <span>{examples.length}</span> : null}
            </button>
          ))}
        </div>
        <label className="search-box">
          <Search size={16} />
          <span className="sr-only">Search examples</span>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Find a little inspiration…"
            type="search"
          />
        </label>
      </div>
      <div className="example-grid">
        {visible.map((e) => (
          <article className="example-card" key={e.id}>
            <div
              className={
                "example-art " + (e.mode === "console" ? "code-art" : "")
              }
              style={{ "--art-accent": e.color } as React.CSSProperties}
            >
              {e.mode === "plot" ? (
                <img
                  src={`${import.meta.env.BASE_URL}examples/${e.id}${dark ? "-dark" : ""}.png`}
                  alt={`${e.title}, rendered with R`}
                  loading="lazy"
                  width="800"
                  height="600"
                />
              ) : (
                <pre aria-label="R source preview">
                  <span>R / SOURCE</span>
                  {e.code.split("\n").slice(0, 7).join("\n")}
                </pre>
              )}
              <span className="art-number">
                {String(examples.indexOf(e) + 1).padStart(2, "0")}
              </span>
            </div>
            <div className="example-info">
              <span className="example-category">{e.category}</span>
              <CardHeading>{e.title}</CardHeading>
              <p>{e.description}</p>
              <button onClick={() => onSelect(e)} aria-label={`Try ${e.title}`}>
                Try this example <ArrowUpRight size={17} />
              </button>
            </div>
          </article>
        ))}
      </div>
      {!visible.length ? (
        <div className="no-results">
          <Search />
          <CardHeading>No examples found.</CardHeading>
          <p>Try “curve”, “data”, or a different category.</p>
          <Button
            variant="outline"
            onClick={() => {
              setQuery("")
              setCategory("All examples")
            }}
          >
            Show all examples
          </Button>
        </div>
      ) : null}
    </section>
  )
}
function EmbedSection() {
  const [tab, setTab] = useState<keyof typeof codeSamples>("Browser"),
    [copied, setCopied] = useState(false)
  return (
    <section id="embed" className="section embed-section">
      <div className="embed-copy">
        <span className="eyebrow">04 / TAKE IT WITH YOU</span>
        <h2>
          Your interface.
          <br />
          <em>R underneath.</em>
        </h2>
        <p>
          Use R for the analysis instead of rebuilding statistical tools in
          JavaScript. Keep the interface you love, and bring the same runtime to
          the browser or a native mobile app.
        </p>
        <div className="platform-list">
          <span>
            <Globe2 />
            Browser · WebAssembly
          </span>
          <span>
            <Sparkles />
            Local AI · R as a tool
          </span>
          <span>
            <Smartphone />
            Mobile · Kotlin & Swift bindings
          </span>
        </div>
        <a className="text-link" href={`${github}/tree/main/crates/r-embed`}>
          Explore the embedding APIs <ArrowUpRight size={17} />
        </a>
      </div>
      <div className="embed-code">
        <div
          className="embed-tabs"
          role="group"
          aria-label="Embedding platform"
        >
          {Object.keys(codeSamples).map((key) => (
            <button
              key={key}
              aria-pressed={tab === key}
              onClick={() => {
                setTab(key as keyof typeof codeSamples)
                setCopied(false)
              }}
            >
              {key}
            </button>
          ))}
          <Button
            variant="ghost"
            aria-label="Copy embedding code"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(codeSamples[tab])
                setCopied(true)
              } catch {
                setCopied(false)
              }
            }}
          >
            {copied ? <Check /> : <Copy />}
          </Button>
        </div>
        <pre>{codeSamples[tab]}</pre>
        <div className="embed-code-footer">
          <Terminal size={14} />
          <span>
            {tab === "Mobile"
              ? "Rust host example · bindings in r-uniffi"
              : tab === "Local AI"
                ? "Integration pattern · connect your preferred model"
                : "Generated wasm-bindgen module · run in a worker"}
          </span>
        </div>
      </div>
    </section>
  )
}
export default function App({ page = "home" }: { page?: Page | "missing" }) {
  const home = page === "home"
  const dark = useDarkTheme()
  const [automatic, setAutomatic] = useState(true)
  const [input, setInput] = useState<PlaygroundInput>({
    code: examples[0].code,
    mode: "plot",
    exampleId: "loess",
    key: 0,
  })
  const exampleId = useSyncExternalStore(
    subscribeLocation,
    () => new URLSearchParams(window.location.search).get("example"),
    () => null
  )
  const selected =
    page === "editor" && input.key === 0
      ? examples.find((item) => item.id === exampleId)
      : undefined
  const editorInput = selected
    ? {
        code: getExampleCode(selected, dark),
        mode: selected.mode,
        exampleId: selected.id,
        key: selected.id,
      }
    : input
  function selectExample(example: Example) {
    if (page === "examples") {
      window.location.href = `${pageHref("editor")}?example=${encodeURIComponent(example.id)}`
      return
    }
    setInput({
      code: getExampleCode(example, dark),
      mode: example.mode,
      exampleId: example.id,
      key: Date.now(),
    })
    document
      .getElementById("playground")
      ?.scrollIntoView({ behavior: "smooth" })
  }
  return (
    <>
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>
      <header className="site-header">
        <a href={pageHref("home")} className="wordmark" aria-label="Rove home">
          <span className="brand-mark">
            R<span>↗</span>
          </span>
          <span>
            rove<span className="wordmark-dot">.</span>
          </span>
        </a>
        <NavigationMenu
          aria-label="Main navigation"
          className="site-navigation"
        >
          <NavigationMenuList>
            {(["editor", "console"] as const).map((key) => (
              <NavigationMenuItem className="site-navigation-direct" key={key}>
                <NavigationMenuLink
                  className="site-navigation-link"
                  href={pageHref(key)}
                  active={page === key}
                  aria-current={page === key ? "page" : undefined}
                >
                  {key === "editor" ? (
                    <Code2 size={16} aria-hidden="true" />
                  ) : (
                    <Terminal size={16} aria-hidden="true" />
                  )}
                  {key === "editor" ? "Editor" : "Console"}
                </NavigationMenuLink>
              </NavigationMenuItem>
            ))}
            <NavigationMenuItem>
              <NavigationMenuTrigger className="site-navigation-trigger">
                <Compass size={16} aria-hidden="true" />
                Explore
              </NavigationMenuTrigger>
              <NavigationMenuContent className="site-navigation-content">
                {(Object.keys(pages) as Page[]).map((key) => (
                  <NavigationMenuLink
                    key={key}
                    href={pageHref(key)}
                    className="site-navigation-item"
                    active={page === key}
                    aria-current={page === key ? "page" : undefined}
                  >
                    <span>{pages[key].label}</span>
                    {page === key && <Check size={14} aria-hidden="true" />}
                  </NavigationMenuLink>
                ))}
              </NavigationMenuContent>
            </NavigationMenuItem>
          </NavigationMenuList>
        </NavigationMenu>
        <div className="header-actions">
          <ThemeToggle />
          <a
            className="github-link"
            href={github}
            aria-label="View Rove on GitHub"
          >
            <svg
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="currentColor"
              aria-hidden="true"
            >
              <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.08 3.29 9.39 7.86 10.91.58.11.79-.25.79-.56v-2.23c-3.2.7-3.88-1.36-3.88-1.36-.52-1.33-1.28-1.68-1.28-1.68-1.05-.72.08-.71.08-.71 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.55-.29-5.23-1.28-5.23-5.69 0-1.26.45-2.29 1.18-3.09-.12-.29-.51-1.46.11-3.05 0 0 .96-.31 3.16 1.18a11 11 0 0 1 5.76 0c2.2-1.49 3.16-1.18 3.16-1.18.62 1.59.23 2.76.11 3.05.74.8 1.18 1.83 1.18 3.09 0 4.42-2.69 5.4-5.25 5.68.41.36.78 1.06.78 2.14v3.24c0 .31.21.68.79.56A11.5 11.5 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z" />
            </svg>
            <span>View on GitHub</span>
            <ArrowUpRight size={14} />
          </a>
        </div>
      </header>
      <main
        id="main-content"
        className={home ? undefined : `focused-page page-${page}`}
      >
        {!home && page !== "editor" && page !== "console" && (
          <div className="page-intro">
            <a href={pageHref("home")}>Rove /</a>
            <h1>
              {page === "missing"
                ? "A little off the path."
                : pages[page].label}
            </h1>
            <p>
              {page === "missing"
                ? "This page doesn’t exist. Open the editor or explore the examples to find your next idea."
                : pages[page].description}
            </p>
          </div>
        )}
        {page === "missing" && (
          <a className="primary-link" href={pageHref("editor")}>
            Open the editor <ArrowRight size={18} />
          </a>
        )}
        {page === "compatibility" && <CompatibilityPage />}
        {page === "console" && <RConsole />}
        {page === "editor" && (
          <h1 className="editor-page-heading">Your space for a little R.</h1>
        )}
        {home && (
          <>
            <section className="hero">
              <div className="hero-copy">
                <div className="hero-kicker">
                  <span className="tiny-orbit" />
                  R, REBUILT IN RUST. READY FOR ANYWHERE.
                </div>
                <h1>
                  R belongs
                  <br />
                  <em>everywhere.</em>
                </h1>
                <p>
                  The joy of R, right in your browser. Explore data, make
                  beautiful plots, and give your local AI a statistical
                  superpower.
                </p>
                <div className="hero-actions">
                  <a
                    className="primary-link"
                    href={home ? "#playground" : pageHref("editor")}
                  >
                    Make something <ArrowRight size={18} />
                  </a>
                  <a
                    className="secondary-link"
                    href={home ? "#examples" : pageHref("examples")}
                  >
                    Take a look around <ArrowDown size={16} />
                  </a>
                </div>
                <div className="hero-assurances">
                  <span>
                    <Check size={13} />
                    No installation
                  </span>
                  <span>
                    <Check size={13} />
                    Runs locally
                  </span>
                  <span>
                    <Check size={13} />
                    Open source
                  </span>
                </div>
              </div>
              <div className="hero-visual">
                <div className="plot-window">
                  <div className="plot-window-top">
                    <span className="window-dots">
                      <i />
                      <i />
                      <i />
                    </span>
                    <span>a_little_less_noise.R</span>
                    <span className="plot-window-badge">
                      <span /> MADE WITH R
                    </span>
                  </div>
                  <img
                    src={`${import.meta.env.BASE_URL}examples/loess${dark ? "-dark" : ""}.png`}
                    width="800"
                    height="600"
                    alt="A LOESS curve following noisy observations, generated with R"
                    fetchPriority="high"
                  />
                  <div className="plot-window-bottom">
                    <span>80 observations. One good hunch.</span>
                    <span>loess(y ~ x)</span>
                  </div>
                </div>
                <div className="floating-note">
                  <span className="note-spark">✳</span>
                  <span>
                    Yes, this runs
                    <br />
                    <em>in your browser.</em>
                  </span>
                </div>
              </div>
            </section>
            <div
              className="runtime-destinations"
              aria-label="Runtime capabilities"
            >
              <div className="destination-heading">
                <strong>Take R with you.</strong>
                <span>Rust at the core · Vello graphics</span>
              </div>
              <div className="destination-list">
                <a href={home ? "#playground" : pageHref("editor")}>
                  <Globe2 size={22} />
                  <strong>Web</strong>
                  <span>WebAssembly</span>
                </a>
                <a href={home ? "#embed" : pageHref("embed")}>
                  <Smartphone size={22} />
                  <strong>Mobile</strong>
                  <span>Swift & Kotlin</span>
                </a>
                <a href={pageHref("ai")}>
                  <Sparkles size={22} />
                  <strong>Local AI</strong>
                  <span>Your model, your device</span>
                </a>
              </div>
            </div>
          </>
        )}
        {(home || page === "editor") && (
          <Playground
            key={editorInput.key}
            input={editorInput}
            onSelect={selectExample}
            automatic={automatic}
            onAutomaticChange={setAutomatic}
            compact={!home}
          />
        )}
        {(home || page === "examples") && (
          <Gallery onSelect={selectExample} standalone={!home} />
        )}
        {page === "ai" && (
          <section id="local-ai" className="section local-ai-section">
            <div className="section-heading">
              <div>
                <span className="eyebrow">03 / A BETTER TOOL FOR YOUR AI</span>
                <h2>
                  Let your AI think.
                  <br />
                  <em>Let R do the math.</em>
                </h2>
              </div>
              <p>
                Ollama drafts the code on your computer. Review it, then let R
                compute the answer.
              </p>
            </div>
            <LocalAI
              onUseCode={(code) => {
                setInput({
                  code,
                  mode: /\b(plot|hist|barplot|boxplot|grid\.|text\(|lines\()/.test(
                    code
                  )
                    ? "plot"
                    : "console",
                  key: Date.now(),
                })
                document
                  .getElementById("playground")
                  ?.scrollIntoView({ behavior: "smooth" })
              }}
            />
          </section>
        )}
        {page === "ai" && (
          <Playground
            key={editorInput.key}
            input={editorInput}
            onSelect={selectExample}
            automatic={automatic}
            onAutomaticChange={setAutomatic}
            compact
          />
        )}
        {(home || page === "embed") && <EmbedSection />}
        {home && (
          <section className="closing-note">
            <span className="eyebrow">
              A SMALL RUNTIME. AN OPEN INVITATION.
            </span>
            <h2>
              Go on. <em>Follow that hunch.</em>
            </h2>
            <p>
              Make something useful. Make something strange. It all starts with
              a little R.
            </p>
            <a
              className="primary-link"
              href={home ? "#playground" : pageHref("editor")}
            >
              Back to the playground <ArrowUpRight size={18} />
            </a>
          </section>
        )}
      </main>
      <footer>
        <a className="wordmark" href={pageHref("home")}>
          rove.
        </a>
        <p>R at heart. Rust underneath. Yours to explore.</p>
        <div>
          <a href={github}>
            Source <ArrowUpRight size={12} />
          </a>

        </div>
        <nav className="footer-sitemap" aria-label="Site map">
          {(Object.keys(pages) as Page[]).map((key) => (
            <a
              key={key}
              href={pageHref(key)}
              aria-current={page === key ? "page" : undefined}
            >
              {pages[key].label}
            </a>
          ))}
        </nav>
      </footer>
      <Toaster />
    </>
  )
}

function subscribeLocation() {
  return () => {}
}
