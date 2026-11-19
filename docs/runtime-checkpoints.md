# Runtime checkpoints

Record the source commit, platform, execution profile and artifact path for each
run. Keep a completed historical result until the same gate has run again.
Use Beads to track failures and missing coverage.

| Gate | Measure | Verified reference |
| --- | --- | --- |
| Pinned GNU R conformance | Passes, failures, timeouts, unattempted cases and verdict changes on unchanged fixtures | 1,261/1,261 passed at `90588275`. [Receipt](ci-checkpoints/gnu-conformance-complete-2026-10-25.json) |
| Unmodified GNU R drivers | Whole and curated results, with skipped and unattempted drivers counted explicitly | 19 passed, 20 failed and 31 skipped whole drivers at `ceeb9571`. All 15 curated slices passed. [Receipt](ci-checkpoints/conformance-browser-ceeb9571.json) |
| Safety | Public API audit, targeted strict Miri checks, and recovery under the original memory and node limits | [Public API audit and strict Miri scope](safe-api-audit.md) |
| Production Wasm | Optimized bytes and SHA-256, public runtime contracts, and Chromium pass/fail/flaky counts under unchanged deadlines | 290 public contracts and 290 recovery checks at `90588275`. [Receipt](ci-checkpoints/gnu-conformance-complete-2026-10-25.json) |

The [capability inventory](capability-evidence.md) gives the current fixture
and disposition counts. The [GNU R API inventory](gnu-r-api-inventory.md)
tracks native registration coverage separately.

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
    docs/ci-checkpoints/gnu-conformance-complete-2026-10-25-cases.json \
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
the scope of any strict Miri run.

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
