import { ArrowUpRight } from "lucide-react"
import { pageHref } from "@/pages"
const contracts = [
  [
    "Language & data",
    "Vectors, data frames, functions, control flow, and a tested subset of imported GNU bytecode: constants, argument lookup, conditional branches, arithmetic and comparisons, guarded math operations, bounded for loops, calls with named, lazy arguments, and enclosing-scope assignment.",
    "Bytecode/compiler execution and serialization, full S3/S4 dispatch, evaluator edge cases, and locale behavior remain incomplete.",
  ],
  [
    "Statistics",
    "Seeded random distributions, FFT and column-wise mvfft, real QR decomposition with rank detection and Q/R factor extraction, linear algebra with faer, LOESS fitting and prediction, and vector Pearson/Spearman correlation.",
    "Coverage is tested operation by operation; passing examples does not establish every numerical or statistical contract.",
  ],
  [
    "Graphics",
    "Base plots, logarithmic abline coordinates, grid viewports, primitive grob geometry edits, mathematical labels, portable PNG output with Vello, and plot layers retained between console commands.",
    "Advanced grid editing and units, device lifecycle, logarithmic axes, patterns/masks, and exact GNU R font typography remain incomplete. The website uses the CPU renderer; GPU integration is a separate API.",
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
          The browser runs R in a worker with a 15-second console timeout (20
          seconds in the playground), a 64 MiB object arena budget, bounded
          result export, and a 256 MiB ceiling on Wasm linear memory. Console
          capture is limited to 1 MiB, and retained interactive graphics use a
          16 MiB accounting budget. Browser rendering and local AI models use
          separate memory; native applications must configure their own resource
          limits.
        </p>
        <p>
          The checked-in inventory contains 636 curated oracle comparisons, but
          only 1 of 70 whole upstream files is marked passing; 9 are expected
          failures and 60 are skipped. Seven packages have selected probes, not
          complete compatibility. These are coverage declarations, not fresh
          test results or a percentage of R implemented.
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
