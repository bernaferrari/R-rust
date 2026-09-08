import { ArrowUpRight } from "lucide-react"
import { pageHref } from "@/pages"
const contracts = [
  [
    "Language & data",
    "Vectors, data frames, functions, control flow and supported base/statistics operations.",
    "The full GNU R API, compiler behavior and locale support are still incomplete.",
  ],
  [
    "Statistics",
    "Seeded random numbers, supported distributions, linear algebra with faer, and LOESS fitting and prediction.",
    "Coverage is tested operation by operation. Passing examples does not establish every numerical or statistical contract.",
  ],
  [
    "Graphics",
    "Base plots, grid viewports and grobs, mathematical labels, and portable PNG output with Vello.",
    "Advanced grid semantics and exact GNU R font typography remain incomplete. The website uses the CPU renderer; GPU integration is a separate API.",
  ],
  [
    "Packages",
    "The runtime's implemented library surface and supported R source packages.",
    "Arbitrary CRAN packages, native extensions and serialized package data are not generally supported in the browser.",
  ],
  [
    "Embedding",
    "Owned Rust values, session handles, Wasm workers, and Swift/Kotlin bindings.",
    "APIs are experimental. Mobile host integration and platform-specific graphics need application-level work.",
  ],
] as const
export function CompatibilityPage() {
  return (
    <article className="compatibility-page">
      <div className="compatibility-summary">
        <span className="eyebrow">THE SHORT VERSION</span>
        <h2>
          A useful R runtime.
          <br />
          <em>Still becoming R.</em>
        </h2>
        <p>
          Rove is an experimental Rust port of R. The examples show working
          contracts you can try today. It is not yet a drop-in replacement for
          GNU R.
        </p>
        <a className="text-link" href={pageHref("examples")}>
          Explore working examples <ArrowUpRight size={16} />
        </a>
      </div>
      <div className="contract-list">
        {contracts.map(([name, supported, limits]) => (
          <section key={name}>
            <h2>{name}</h2>
            <p>{supported}</p>
            <p>
              <strong>Still to do.</strong> {limits}
            </p>
          </section>
        ))}
      </div>
      <section className="compatibility-summary">
        <span className="eyebrow">SAFETY & EVIDENCE</span>
        <h2>Tested, with boundaries.</h2>
        <p>
          The embedding API keeps raw interpreter objects out of host code.
          Internal unsafe code still needs wider auditing; bounded Miri and GC
          stress tests do not prove the entire interpreter sound.
        </p>
        <p>
          The browser runs R in a worker with a 20-second timeout, a 64 MiB
          object arena budget, bounded result export, and a 256 MiB ceiling on
          Wasm linear memory. Console capture is limited to 1 MiB. Browser
          rendering and local AI models use separate memory; native applications
          must configure their own resource limits.
        </p>
        <p>
          Compatibility tests compare curated cases against an exact GNU R
          source revision. Counts measure evidence, not a percentage of R
          implemented.
        </p>
        <a
          className="text-link"
          href="https://github.com/bernaferrari/R-rust/blob/main/docs/conformance.md"
        >
          Read the test contract <ArrowUpRight size={16} />
        </a>
      </section>
    </article>
  )
}
