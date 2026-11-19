# Runtime checkpoints

Record the source commit, platform, execution profile and artifact path for each
run. Keep a completed historical result until the same gate has run again.
Use Beads to track failures and missing coverage.

| Gate | Measure | Verified reference |
| --- | --- | --- |
| Pinned GNU R conformance | Passes, failures, timeouts, unattempted cases and verdict changes on unchanged fixtures | 1,270/1,270 passed at `b6a90a8f`. [Receipt](ci-checkpoints/gnu-conformance-main-b6a90a8f.json) |
| Unmodified GNU R drivers | Whole and curated results, with skipped and unattempted drivers counted explicitly | 22 passed, 18 failed and 30 skipped whole drivers at `1faa7f2a`. All 15 curated slices passed. [Receipt](ci-checkpoints/upstream-main-1faa7f2a.json) |
| Safety | Public API audit, targeted strict Miri checks, and recovery under the original memory and node limits | [Public API audit and strict Miri scope](safe-api-audit.md) |
| Production Wasm | Optimized bytes and SHA-256, public runtime contracts, and Chromium pass/fail/flaky counts under unchanged deadlines | 328 public contract executions at `eddd0913`. Chromium passed 77/77 at `c7fa992f` on macOS arm64 and at `ad04d62f` on hosted Linux. |

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
