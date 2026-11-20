# Runtime checkpoints

Record the source commit, platform, execution profile and artifact path for each
run. Keep a completed historical result until the same gate has run again.
Use Beads to track failures and missing coverage.

| Gate | Measure | Verified reference |
| --- | --- | --- |
| Pinned GNU R conformance | Passes, failures, timeouts, unattempted cases and verdict changes on unchanged fixtures | 1,281/1,281 passed at `eddd0913`. [Receipt](ci-checkpoints/gnu-conformance-main-eddd0913.json) |
| Unmodified GNU R drivers | Whole and curated results, with skipped and unattempted drivers counted explicitly | 22 passed, 18 failed and 30 skipped whole drivers at `1faa7f2a`. All 15 curated slices passed. [Receipt](ci-checkpoints/upstream-main-1faa7f2a.json) |
| Safety | Public API audit, targeted strict Miri checks, and recovery under the original memory and node limits | [Public API audit and strict Miri scope](safe-api-audit.md) |
| Production Wasm | Optimized bytes and SHA-256, public runtime contracts, and Chromium pass/fail/flaky counts under unchanged deadlines | 332 public contract executions at `87328a0c`. Chromium passed 77/77 at `c7fa992f` on macOS arm64 and at `ad04d62f` on hosted Linux. |

The [capability inventory](capability-evidence.md) gives the current fixture
and disposition counts. The [GNU R API inventory](gnu-r-api-inventory.md)
tracks native registration coverage separately.
The [everyday-analysis scorecard](everyday-analysis.md) tracks which complete
user workflows have passed on native Rport and production Wasm.

## November 19, 2026 checkpoint

- The full pinned GNU run at `dfc8abe5` passed 1,263 of 1,264 cases with zero
  timeouts or skips. All three added fixtures passed. The comparison found
  one changed verdict on an unchanged fixture. [Complete case receipt](ci-checkpoints/gnu-conformance-complete-2026-11-19-cases.json)
- The single failure was `600_smooth`. At `c5007ff3`, its unchanged fixture
  matches GNU again. A direct comparison also matched the length, names and
  values of the native result for each of the six smoother modes.
- All 15 curated upstream slices passed at `dfc8abe5` with the graphics
  runtime and native package policy.
- The TRE suite passed 100 tests. Targeted strict Miri checks passed for
  zero-headroom diagnostics and TRE stack owner revocation. The original warm
  S4 check passed in 12.27 seconds with native packages and 12.00 seconds with
  portable packages under its 60-second, 64 MiB and 500,000-node limits.
- The arm64 Android crates passed cross-compilation. The separate global-state
  audit reported 75 unclassified declarations.

- At `56952f29`, the Gradle production bundle gate accepted 25,850,514 bytes
  of Rust runtime assets. The module passed all 77 Chromium tests and 290
  public Wasm contracts on macOS arm64. The Chromium run includes the
  original 29-case FFT/RNG aggregate and all six S4 workflows. The
  size-only Cargo level `s` experiment missed a fresh S4 deadline. The
  release wrapper uses level `2`.
- At `e3e0859a`, the Gradle production bundle gate accepted 25,916,546 bytes
  of Rust runtime assets, leaving 297,854 bytes under 25 MiB. The optimized
  module SHA-256 is
  `5252ab439471f5915243c0194473ea133e5ad9aa841d29cf02059986d93830cd`.
  It passed all 294 public Wasm contracts and both unchanged warm Chromium
  S4 workflows in 10.02 and 10.32 seconds. Those checks include the new
  recursive `anyNA()` and unchanged `smooth()` fixtures.
- Pairlist snapshot admission passed 17 native tests and four targeted
  strict Miri checks. On the same runtime, the original release-mode S4
  check passed in 5.79 seconds with native packages and 7.83 seconds with
  portable packages. The `anyNA()` fixtures passed under both package policies.

## Main verification

- At `b64ff134`, optimizer scratch uses the session’s transient
  allocator and nested bounded fits each own their continuation state.
  Native and production Wasm completed nested fits and recovered after
  oversized input and callback errors. The optimized module passed all
  332 Wasm contracts and the dependency and S4 gate. The Workbench bundle
  uses 24,018,335 bytes and leaves 2,196,065 bytes below 25 MiB. The
  existing bounded `nlminb()` reporting gap is tracked in
  `rport-ckbed.23`. [Optimizer receipt](ci-checkpoints/optim-workspace-b64ff134.json)

- At `720ddfb7`, `optim(method = "BFGS")` and the package numerical
  routines can use the same slice-based optimizer. It charges the
  temporary matrix and vectors to the active session before invoking
  the objective. Pinned GNU R, native Rport and production Wasm matched
  the four fit results and two error messages. Wasm also refused a
  5,000-parameter fit before evaluating the objective and completed a
  smaller fit in the same session. All 332 Wasm contracts and the
  dependency and S4 gate passed. The Workbench bundle leaves 2,146,155
  bytes below 25 MiB. [BFGS and production receipt](ci-checkpoints/bfgs-720ddfb7.json)

- At `d55ab963`, 17 of 20 everyday workflows passed on both targets.
  Native and Wasm grid replay matched 14 rendered PDFs from pinned GNU R.
  All 18 selected strict native cases, 332 Wasm contract runs, the
  dependency and S4 gate, and seven Chromium deadline checks passed.
  The Workbench bundle uses 24,055,796 bytes and leaves 2,158,604 bytes
  below 25 MiB. [Production receipt](ci-checkpoints/wasm-bundle-d55ab963.json)

- At `1790c295`, the source package loader checks versioned `Imports`
  that are used by `NAMESPACE` and versioned `Depends` at attachment.
  The pinned R6 workflow verified refusal, retry, and loaded identity on
  native Rport and exact production Wasm. Recursive package lookup through
  a one-off `lib.loc` remains a compatibility gap. The release passed 18
  selected strict native cases, 332 production Wasm contracts, the separate
  dependency and S4 gate, and seven Chromium deadline checks. The Workbench
  bundle uses 24,117,846 bytes and leaves 2,096,554 bytes below 25 MiB.
  [Production receipt](ci-checkpoints/wasm-bundle-1790c295.json)

- At `fa0c3340`, `loadNamespace()` checked an explicit library and rejected
  incompatible R6 version requests before registration. A loaded R6 namespace
  retained its version and identity after the library search path changed.
  The pinned package workflow repeated all checks on GNU R, native Rport and
  exact production Wasm. The original tools function also matched pinned GNU
  direct, recursive and reverse dependency queries.
  [Production receipt](ci-checkpoints/wasm-bundle-fa0c3340.json)

- At `df6decc7`, production Wasm decoded the pinned datasets through
  the same bounded codec as other lazy-loaded R records. The contract
  checked all 108 dataset objects and 592,931 serialized graph bytes.
  The exact Workbench bundle grew 52,209 bytes to 24,024,817 bytes, leaving
  2,189,583 bytes below 25 MiB. The report rebuild raised the everyday
  score to 16 of 20 on each target. All 17 selected strict native cases,
  332 production Wasm contract runs, and seven Chromium deadline checks
  passed.
  [Production receipt](ci-checkpoints/wasm-bundle-0ea69c8a.json)

- At `3e5bbcbb`, native and production Wasm verified 15 of 20 everyday
  workflows. The ordinary PDF script produced valid vector output, and
  refusing unsupported raster content preserved the previous output.
  All 17 selected native GNU cases, 332 Wasm contract executions, and
  seven Chromium deadline checks passed.
  [Production receipt](ci-checkpoints/wasm-bundle-3e5bbcbb.json)

- At `87328a0c`, 17 selected cases passed pinned native GNU conformance.
  The exact optimized Wasm module passed 332 contracts across two fresh
  generations and seven Chromium deadline checks. The project workflow
  also verified session-local paths, output connection identity after
  changing directories, rejected traversal, and quota-safe replacement.
  The Workbench bundle uses 24,938,750 bytes and leaves 1,275,650 bytes
  below the 25 MiB cap.
  [Production receipt](ci-checkpoints/wasm-bundle-87328a0c.json)


- At `a0d25066`, all 328 production Wasm contract executions passed. The
  full generated package measured 25,694,029 bytes, leaving 520,371 bytes
  below 25 MiB. The optimized module measured 25,658,995 bytes with SHA-256
  `6184a09e372d4f454e422be814966ec1241ebe81b80fc7db7cf81914e23ddcdc`.
  The exact package passed the pinned R6 analysis and all nine workspace
  interchange combinations. All 12 selected native GNU conformance cases passed.
  [Workflow receipt](ci-checkpoints/everyday-analysis-v1-a0d25066.json)


- All 1,264 pinned GNU R fixtures passed at `1faa7f2a` on macOS arm64.
  There were no timeouts or skips. The corpus and oracle fingerprints match
  the preceding 1,263-pass run. Only `600_smooth` changed verdict.
  [Complete case receipt](ci-checkpoints/gnu-conformance-main-1faa7f2a.json)
- The unmodified upstream run completed all 85 dispositions at the same
  commit with the graphics runtime and native package policy. All 15
  curated slices passed. Of 70 whole drivers, 22 passed, 18 failed the
  strict gate and 30 were declared skips. The strict comparisons include
  process-specific addresses and timing differences as well as runtime
  behavior. [Driver results](ci-checkpoints/upstream-main-1faa7f2a.json)
- Duplication workspace admission at `f25f1244` passed 70 native tests, three
  strict-provenance Miri checks and 294 production Wasm contracts. The original
  warm native S4 checks passed with both package policies in 6.57 and
  9.33 seconds under the original memory, node and time limits.
- At `376e4942`, native and Wasm fixtures matched GNU for `pbeta()` warning
  attribution and POSIXlt differences in explicit time zones. All 18 underflow
  warning lines also matched in the unmodified beta driver. Other numerical and
  timing differences still fail that whole driver. `datetime3.R` advanced from
  subtraction to a separate character-conversion failure. Both tests in the
  original embedding target for missing subscripts and Fisher overflow passed.
- The same runtime sources produced a 25,913,321-byte Wasm module with SHA-256
  `bb90f757ba2ac4ae1ed8daa67b223472e8250574545c319fea2cde9073099228`.
  The package passed 298 public Wasm contracts and all 77 Chromium tests on macOS
  arm64. Both warm Chromium S4 runs completed in 10.58 and 10.44 seconds.
  The optimized native warm S4 checks passed under both package policies in
  6.60 and 7.87 seconds.
- Hosted Chromium at `1faa7f2a` passed 75 of 77 tests. The fresh S4 worker
  and the FFT/RNG aggregate missed their original deadlines. All 29 FFT/RNG
  case bodies completed, using 89.37 seconds of the 90-second aggregate
  budget. [Hosted run](https://github.com/bernaferrari/R-rust/actions/runs/44594984309)
- The full macOS arm64 run at `376e4942` passed 1,266 pinned GNU R fixtures
  with zero failures, skips or timeouts. Both new fixtures passed and every
  unchanged fixture kept its verdict. The public embedding release tests
  selected by CI passed all 84 tests across 33 targets on the same host.
  [Complete case receipt](ci-checkpoints/gnu-conformance-main-376e4942.json)
- At `0a506c11`, symbol comparison reuses validated headers and preserves
  ownership and type checks. The focused native and strict Miri checks passed.
  Six matched pairs with optimized Wasm reduced median Node session startup
  from 3.06 to 2.86 seconds on macOS arm64. All seven original S4 and FFT/RNG
  Chromium checks passed on that host.
- At `a11657b0`, `as.character.POSIXt()` preserves missing values formed by
  numeric date arithmetic. The integrated optimized module was 25,942,717
  bytes with SHA-256
  `3cd3929025a6fb629caad07ebacc7a1a62924ca1519f61153a9e795e9e302845`.
  All 300 public Wasm contracts and their recovery checks passed.
- Hosted Linux at `ad04d62f` passed all 77 Chromium tests under the original
  deadlines, including the original FFT/RNG aggregate in 85.79 seconds.
  The exact GNU gate completed 1,265 cases with one failure because pinned
  GNU on Linux differed from the saved macOS `covratio()` value by a few
  rounding bits. At `d33e8300` each finite value is checked against a relative
  bound of eight times double machine epsilon. The printed output is still
  compared exactly. [Hosted run](https://github.com/bernaferrari/R-rust/actions/runs/44599578151)
- At `b044a4d4`, the datetime source is authenticated against the GNU oracle
  revision. Vector and scalar POSIXlt replacement match GNU across UTC and
  GMT near year 10,000 and at the double-precision boundary. The optimized
  package passed 302 Wasm contracts and their recovery checks. It measured
  25,943,590 bytes with SHA-256
  `e0ff500752bb17c652457bab5d541542b80377d84c7587cc5bb25b558a73fe7c`.
  All 1,268 pinned GNU fixtures passed, including the amended `covratio()`
  fixture and two new date fixtures. There were no verdict changes on unchanged
  fixtures.
- At `eea08fe9`, `sprintf()` recomputes recycling after a coercion shortens
  its longest argument. Coerced values and the result stay rooted across
  allocation and collection. The optimized package passed 304 public contracts
  with recovery checks. The unmodified `reg-tests-1b.R` advanced from the
  formatting assertion to a separate stats namespace version assertion.
- At `b6a90a8f`, native and Wasm agree with the pinned clock fixture. `Sys.Date()`
  reads the owning session's time zone, and the JavaScript host supplies
  current time for `Sys.time()` and time-only `strptime()`. The unmodified
  native `datetime3.R` exits successfully. The strict output comparison still
  differs, including warning attribution and object listing order. The combined
  release build passed 306 public Wasm contracts and their recovery checks. All
  1,270 exact GNU R cases passed without skips or timeouts. Four additions and
  the amended `covratio()` fixture passed. Every unchanged verdict held.
  [Complete case receipt](ci-checkpoints/gnu-conformance-main-b6a90a8f.json)
- At `75c58c7b`, factor subsetting retains the explicit `NA` level when dropping
  unused levels, including in compiled calls. All 14 targeted factor and
  subsetting fixtures matched GNU. The unmodified `reg-tests-1d.R` proceeds to
  a separate `xtabs()` dimension-name mismatch. The 25,969,708-byte release
  package passed all 308 public contracts and their recovery checks. Its
  SHA-256 is `dbd63a7061b9248e02d9dfb96ee52d5f1b9a916df24b725c42d2f82188305af6`.
  That package also passed all seven original Chromium S4 and FFT/RNG tests on
  macOS arm64 under the original deadlines, including the 29-case aggregate.
- The line-table test profile allowed hosted Linux workspace shard 0 to link
  and execute its complete partition. It caught two tests with stale console
  expectations. At `174855f3`, all nine tests in the two affected graphics
  targets pass in the optimized native profile on macOS arm64. Hosted debug
  confirmation is pending. [Hosted shard](https://github.com/bernaferrari/R-rust/actions/runs/44599578151/job/135827127414)
- At `174855f3`, six native release test routines passed the selected probes
  across seven pinned pure-R packages unpacked into a fresh bundled library.
  The screen covers source loading and selected package behavior. General
  package installation and compiled extension workflows need more evidence.
  [Package inventory](../tests/real-packages/manifest.toml)
- At `46d421ff`, browser file writes and copies charge the owning session and
  retain the old file when a replacement is refused. Closed sessions return
  file-operation errors. Wasm upload bounds are checked before data is copied
  into linear memory. All four optimized public file tests, nine store tests
  and the focused strict-provenance Miri test passed. The optimized Wasm module
  passed 312 public contracts with recovery checks. It measured 25,947,736
  bytes with SHA-256
  `449303d18adb733eaee805e67db2b54f98b2d3ad459bc8cd6829ce2ea002ede9`.
- Hosted Linux at `ad04d62f` linked and admitted all nine workspace debug
  partitions. Two passed, two completed with failed tests, and five reached
  the existing partition deadline. [Hosted run](https://github.com/bernaferrari/R-rust/actions/runs/44599578151)
- At `c7fa992f`, seven everyday analysis workflows passed strict native
  conformance and the production Wasm contract runner. Both Wasm session
  generations completed all 161 public contracts and their recovery checks.
  All 77 Chromium tests passed on macOS arm64 with zero skips or flaky tests.
  These checks used the 26,044,676-byte optimized module built from the
  identical runtime sources at `8566c58b`. Its SHA-256 is
  `7c71e037abe5c88c209dfcff29d35d575237c68b540b4e415ff0906892d71345`.
  [Workflow receipt](ci-checkpoints/everyday-analysis-v1-c7fa992f.json)
- At `eddd0913`, the browser reads ordinary gzip files through bounded
  connection snapshots. Strict Miri covered denied allocation, truncated
  gzip, and recovery with the miniz backend. All 328 production contract
  executions passed with the 26,186,041-byte module. Its SHA-256 is
  `2cd9d9b6e582c07abb9d929175363810ef611078809da215aafb8db5fc22a582`.
  At `169031b1` the same runtime also passed the file import, export,
  and reload workflow in independent native and browser sessions.
  [Workflow receipt](ci-checkpoints/everyday-analysis-v1-169031b1.json)
- All 1,281 native cases passed at `eddd0913` with zero timeouts, skips, or
  unattempted cases. All 11 additions passed and unchanged cases kept their
  verdicts against the prior full run at `b6a90a8f`.
  [Complete case receipt](ci-checkpoints/gnu-conformance-main-eddd0913.json)

## Wasm footprint

### Browser analysis tools

The opt-in `browser-analysis-tools` feature replaces six host-oriented
`tools` functions with an error naming the selected profile. These are
the entry points for `R CMD check`, `R CMD build`, `R CMD INSTALL`, the
CRAN incoming checks and their formatter, and `check_packages_in_dir()`.
The remaining 970 records are byte-identical to the full archive.
Package loading, dependency queries and ordinary `tools` helpers use
their existing code. The full archive is the default.

The optimized analysis module uses 23,429,095 bytes, saving 554,207
bytes. The Workbench runtime bundle leaves 2,750,272 bytes below 25 MiB.
The default Wasm module matches the previous release byte for byte.
The analysis build passed all 332 Wasm contract runs, the pinned R6 package
analysis, dependency checks and PNG and PDF export checks. Chromium
verified the error and continued running analysis in the same session.
[Verification receipt](ci-checkpoints/browser-analysis-tools-650ceb87.json)

```sh
RPORT_WASM_FEATURES=browser-analysis-tools \
  scripts/build_wasm_runtime.sh --target web --release --out-dir "$PWD/pkg-analysis"
```

The build assembles the smaller archive from the pinned full archive
and a compact overlay. When updating the pinned `tools` assets, run
`pack_browser_analysis_tools.R` with pinned GNU R and pass the full
asset directory and its `browser-analysis` subdirectory as arguments.
The build detects an overlay generated from a different input archive.

### Size experiments

Disabling file-backed PDF export at `b64ff134` reduced the optimized Wasm
module from 23,983,302 bytes to 23,810,089 bytes, a saving of 173,213
bytes. Both builds ran the same normal optimizer. The smaller build still
evaluated R, rendered PNG previews and supported `pdf(NULL)`. It retained
the 860,516-byte DejaVu archive used by plot previews. The default Wasm
ships the software renderer. WebGPU builds use an opt-in feature, and
the module has no Quartz backend. [PDF size experiment](ci-checkpoints/wasm-pdf-export-ablation-b64ff134.json)

At `d55ab963`, the 24,020,763-byte module contains 14,052,599 bytes of
compiled code and 9,941,817 bytes of initialized data. The exact module
compressed to 11,492,492 bytes with Brotli 1.2.1 at quality 11 and
13,674,858 bytes with gzip at level 9. In a local Chromium 153 run, the
generated loader streamed the Brotli HTTP response through
`WebAssembly.instantiateStreaming` and evaluated `mean(c(2, 4, 6))` in a
new R session. The Workbench size gate continues to count raw assets.
[Module section breakdown](ci-checkpoints/wasm-size-d55ab963.json)

At `fa0c3340`, Workbench packages 24,034,743 raw bytes and leaves 2,179,657
bytes below 25 MiB. The Wasm module contains 14,031,170 bytes of executable
code and 9,942,374 bytes of initialized data. Compressing that exact module
locally produced 11,487,609 bytes with Brotli quality 11 and 13,666,206 bytes
with gzip level 9. Chromium streamed, instantiated and evaluated R on the
same module served locally with Brotli and `application/wasm`. HTTP
compression reduces transfer size. The Workbench gate counts raw assets.
[Production receipt](ci-checkpoints/wasm-bundle-fa0c3340.json)

An offline sweep repacked the same 4,304 GNU records across the seven
bundled package databases. The decoded byte streams and R indexes matched
the existing 64 KiB configuration exactly. These figures count the packed
data, block directories and indexes. The last column measures how many
bytes the decoder would discard to reach a record, across all records.

| Target block size | Raw asset bytes | Change from 64 KiB | 95th percentile prefix bytes |
| --- | ---: | ---: | ---: |
| 32 KiB | 6,528,635 | +229,474 | 28,938 |
| 64 KiB | 6,299,161 | 0 | 58,677 |
| 128 KiB | 6,170,327 | −128,834 | 119,056 |
| 256 KiB | 6,083,776 | −215,385 | 245,072 |
| 512 KiB | 6,049,591 | −249,570 | 487,037 |

Production still uses 64 KiB blocks. The sweep measures bytes and decoder
work. Startup and first-package call timings would need a production build.
The `tools` database, directory and index together occupy 2,071,297 bytes.
A separately cached copy would reduce the initial module, with the same
bytes still counting toward the cap if shipped in the same bundle.

The PDF build uses 23,972,608 bytes in the Workbench production bundle,
leaving 2,241,792 bytes (2.14 MiB) below 25 MiB. A named profile with identical
execution sections attributes 9,236,671 function-body bytes to `rmath`
symbols, including 2,384,498 under `mainutils::essentials`. Those symbols
include inlined dependencies. The profile identifies 37,438 body bytes
under `lzma_rs` symbols and 108,281 under `lzma_rust2` symbols. The optimized
module compressed locally to 12,156,448 bytes with Brotli and 13,656,301
with gzip. The Workbench cap counts all raw runtime assets. A standalone
similar-function merging pass increased the module by 137,463 bytes.
[Code and data breakdown](ci-checkpoints/wasm-code-3e5bbcbb.json)

At `6b6dfaab`, Workbench packages **23,990,595 bytes**, leaving
**2,223,805 bytes (2.12 MiB)** below the 25 MiB ceiling. The bundle is
948,155 bytes smaller than at `87328a0c`. The code section remains
13,991,677 bytes and the data section is 9,937,820 bytes. The Workbench
build logs the full bundle reserve and warns when it drops below 1 MiB.
[Production size and timing receipt](ci-checkpoints/wasm-bundle-6b6dfaab.json)
[Module section breakdown](ci-checkpoints/wasm-size-6b6dfaab.json)

The Wasm images of `stats`, `methods`, `utils`, `graphics`, `grDevices`,
and `stats4` now use the same bounded block decoder as `tools`. All 3,328
newly packed records match the pinned GNU serializations byte for byte.
Native builds use the original GNU databases. `tools` (1.93 MiB), `stats`
(1.48 MiB), and `methods` (0.99 MiB) are the largest packed data payloads.
The existing bundled DejaVu faces add 0.82 MiB. The section report lists
only contiguous embedded files. The bundle receipt also counts the packed
database inputs that Binaryen splits across data segments.

Across seven alternating pairs on macOS arm64, median Node session startup
measured 2.45 seconds before packing and 2.25 seconds afterward. The first
model fit and package calls measured 213 and 214 ms. The exact production
artifact passed 332 public contract executions, the `stats4` maximum-
likelihood contract, and all seven Chromium deadline checks. All 17 selected
strict native cases passed against pinned GNU R.

An extra Binaryen convergence pass on the previous optimized module saved
6,308 bytes. Packing only DejaVu regular and bold would save 363,532 data
bytes and lose the bundled italic faces. The current bundle carries all
four faces and uses the original optimizer invocation.

The optimized `eddd0913` module left **28,359 bytes** under 25 MiB. Its code
section used 14,023,273 bytes and its data section used 12,136,792. The
complete generated Rust package was 26,221,075 bytes, 6,675 over that limit.
The largest exact embedded files were the `tools` (2,360,298 bytes), `stats` (1,857,724),
`methods` (1,361,830), and `utils` (1,043,845) lazy-load databases and the
compressed DejaVu font faces (860,516).
[Baseline size receipt](ci-checkpoints/wasm-size-eddd0913.json)

At `2f30ab7a`, the optimized module used **25,583,620 bytes**, leaving
**630,780 bytes**. It passed all 328 production contract executions and
recovery checks. The packers verified byte-for-byte equality for all 597
timezone names and all 1,721 bundled example aliases. The native portable
example test matched GNU prompts and output. Examples decompress on lookup.
[Packed size receipt](ci-checkpoints/wasm-size-2f30ab7a.json)

The named profiling build at `2f30ab7a` attributed about 9.15 MB of optimized
function bodies to `rmath` symbols and 0.53 MB to `faer`. The production
optimizer verified that the profile and shipping module have identical
standard sections. The attribution includes code inlined into those symbols.
One of seven browser budget checks timed out while running the packed module.
Two isolated repeats on each of the baseline and packed modules passed that
cold S4 check with the original deadline.

With the workspace safety change at `56a3fe41`, the full generated Rust
package is **25,642,009 bytes**, leaving **572,391 bytes** below the 25 MiB
budget. The Wasm module itself is 25,606,975 bytes. That independently
built module passed all 328 production contract executions and workspace
interchange in both directions with GNU R.
[Workspace build size receipt](ci-checkpoints/wasm-size-56a3fe41.json)

At `bc598c83`, the production Workbench budget gate measured **24,914,663
bytes**, leaving **1,299,737 bytes (1.24 MiB)** below 25 MiB. The generated
package is 779,365 bytes smaller than at `a0d25066`. The optimized module is
24,879,630 bytes with SHA-256
`99d554d05c4839f244f0f04106cd5654acab510b1b256c6b849f80cb5258c447`.
[Module size receipt](ci-checkpoints/wasm-size-bc598c83.json)
[Gradle bundle receipt](ci-checkpoints/wasm-bundle-bc598c83.json)

Wasm packs all 976 `tools` records in independently compressed blocks.
The records and block directory use 2,029,333 bytes, compared with
2,360,298 bytes for the previous database. All variable and reference
records matched byte for byte. Native Rport still reads the original GNU
database. Build-time compression reduced the 118 embedded base R source
files from 730,596 to 236,647 bytes before linking. Each source is decoded
as needed and its original bytes reach the parser.

Across seven alternating pairs on macOS arm64 with the same Node version,
median session initialization was 2.80 seconds on the previous package and
2.90 seconds on the new one. Median first use of `tools` was 199 and 211 ms.
The repeat call took 34 ms on each package. Those samples combine the base
source, package data and timezone changes.

The same module passed all 328 production contract executions and all
seven original Chromium deadline checks. The focused native tests passed
for packed range reads, budget cleanup, timezone retry, and base initialization.
The timezone fix also passed both strict-provenance Miri tests. In production
Wasm, a known timezone reports a resource error when the lookup exceeds
the session budget. Freeing memory lets the same session retry successfully.

```bash
python3 scripts/wasm_size_report.py "$report_dir/pkg"
```

The report gives the module and full package sizes. It counts exact contiguous
asset matches in the module’s data section. A file split by the optimizer or
transformed during linking can contribute bytes without appearing in the list.

## Re-run GNU R parity

Run from a clean checkout. The pinned oracle installer prints its binary
directory on the final line. Use a fresh report directory for each run.

```bash
oracle_bin="$(scripts/install_r_oracle.sh | tail -n 1)"
export PATH="$oracle_bin:$PATH"
export RPORT_REQUIRE_PINNED_ORACLE=1 RPORT_CONFORMANCE_PROFILE=release
report_dir="$(mktemp -d)"
commit="$(git rev-parse HEAD)"

for shard in 0 1 2 3 4 5; do
    scripts/conformance_parity.sh --check --strict \
        --shard-index "$shard" --shard-count 6 --report "$report_dir/shard-$shard"
done

python3 scripts/conformance_shards.py --input-dir "$report_dir" --shard-count 6 \
    --source-commit "$commit" --json "$report_dir/union.json" \
    --markdown "$report_dir/union.md"
python3 scripts/conformance_delta.py \
    docs/ci-checkpoints/gnu-conformance-main-1faa7f2a.json \
    "$report_dir/union.json"
```

The union verifies shard coverage and records semantic failures. The comparison
takes complete, pinned strict unions produced from immutable Git checkouts.
It lists added, removed and edited fixtures separately from verdict changes on
unchanged fixtures, and requires matching oracle and execution settings.

Run the slower unmodified drivers with the CI runtime and package policy.

```bash
scripts/upstream_core_slices.sh --strict --timeout 1800 --runtime-profile graphics \
    --package-policy native --suite all --report "$report_dir/upstream"
```

## Re-run safety and Wasm gates

Use the [release gate](release-gate.md) for the workspace and platform checks.
For a runtime change, exercise its denial and recovery paths directly and record
the scope of any strict Miri run. The Wasm release wrapper defaults to Cargo
optimization level `2` before Binaryen optimization. It honors an explicit
caller override.

```bash
scripts/audit_safe_api.sh
scripts/build_wasm_runtime.sh --target web --release --out-dir "$report_dir/pkg"
node tests/browser/portable-package-contracts.mjs "$report_dir/pkg"
(
    cd website
    pnpm install --frozen-lockfile
    export RPORT_WASM_PKG="$report_dir/pkg"
    pnpm build
    CI=1 \
        PLAYWRIGHT_JSON_OUTPUT_FILE="$report_dir/playwright.json" \
        pnpm test:e2e --reporter=line,json
)
```

Install Chromium through `pnpm exec playwright install chromium` in
`website` before running the browser suite. Keep the optimizer receipt
`rust-runtime-optimization.json` with the Playwright JSON. Use repeated runs
on the affected platform when evaluating a deadline regression.
