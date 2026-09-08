import { useState } from "react"
import {
  ArrowDown,
  ArrowUpRight,
  ArrowRight,
  Check,
  Copy,
  GitBranch,
  Globe2,
  Search,
  Smartphone,
  Sparkles,
  Terminal,
  Zap,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Playground, type PlaygroundInput } from "@/components/Playground"
import { LocalAI } from "@/components/LocalAI"
import { examples, categories, type Example } from "@/data/examples"
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
function Gallery({ onSelect }: { onSelect: (example: Example) => void }) {
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
          Twelve ideas to start from.
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
                  src={`${import.meta.env.BASE_URL}examples/${e.id}.png`}
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
              <h3>{e.title}</h3>
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
          <h3>No examples found.</h3>
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
export default function App() {
  const [input, setInput] = useState<PlaygroundInput>({
    code: examples[0].code,
    mode: "plot",
    exampleId: "loess",
    key: 0,
  })
  function selectExample(example: Example) {
    setInput({
      code: example.code,
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
      <a className="skip-link" href="#playground">
        Skip to playground
      </a>
      <header className="site-header">
        <a href="#" className="wordmark" aria-label="Rove home">
          <span className="brand-mark">
            R<span>↗</span>
          </span>
          <span>
            rove<span className="wordmark-dot">.</span>
          </span>
        </a>
        <nav aria-label="Main navigation">
          <a href="#playground">Playground</a>
          <a href="#examples">Examples</a>
          <a href="#local-ai">Local AI</a>
          <a href="#embed">For builders</a>
        </nav>
        <a
          className="github-link"
          href={github}
          aria-label="View Rove on GitHub"
        >
          <GitBranch size={17} />
          <span>View on GitHub</span>
          <ArrowUpRight size={14} />
        </a>
      </header>
      <main>
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
              The joy of R, right in your browser. Explore data, make beautiful
              plots, and give your local AI a statistical superpower.
            </p>
            <div className="hero-actions">
              <a className="primary-link" href="#playground">
                Make something <ArrowRight size={18} />
              </a>
              <a className="secondary-link" href="#examples">
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
                src={`${import.meta.env.BASE_URL}examples/loess.png`}
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
        <div className="capability-strip">
          <span>
            <Zap size={17} />
            Rust at the core
          </span>
          <span>
            <Globe2 size={17} />
            WebAssembly in your browser
          </span>
          <span>
            <ChartIcon />
            Vello-powered graphics
          </span>
          <span>
            <Sparkles size={17} />
            Built for local AI
          </span>
        </div>
        <Playground key={input.key} input={input} onSelect={selectExample} />
        <Gallery onSelect={selectExample} />
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
              A local language model drafts the code. R computes the answer. Try
              a browser model or connect Ollama on your computer.
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
        <EmbedSection />
        <section className="closing-note">
          <span className="eyebrow">A SMALL RUNTIME. AN OPEN INVITATION.</span>
          <h2>
            Go on. <em>Follow that hunch.</em>
          </h2>
          <p>
            Make something useful. Make something strange. It all starts with a
            little R.
          </p>
          <a className="primary-link" href="#playground">
            Back to the playground <ArrowUpRight size={18} />
          </a>
        </section>
      </main>
      <footer>
        <a className="wordmark" href="#">
          rove.
        </a>
        <p>R at heart. Rust underneath. Yours to explore.</p>
        <div>
          <a href={github}>
            Source <ArrowUpRight size={12} />
          </a>
          <a href={`${github}/blob/main/docs/loess-and-portable-graphics.md`}>
            Compatibility
          </a>
          <a href={`${github}/blob/main/COPYING`}>GPL license</a>
        </div>
      </footer>
    </>
  )
}
function ChartIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 18 18"
      fill="none"
      aria-hidden="true"
    >
      <path
        d="M2 3v12h14M4 11l3-4 3 2 5-6"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}
