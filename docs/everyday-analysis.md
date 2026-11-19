# Everyday analysis readiness

The goal is to run the scripts, data and packages people need for routine
analysis with behavior they can verify against GNU R. The versioned inventory
includes file and package workflows alongside statistical checks.
[Workflow inventory](../tests/everyday-analysis/workflows.json)

Version 1 has 20 workflows. The 90% checkpoint requires at least 18 passing
workflows on each target. The 95% checkpoint requires 19. Every declared
workflow stays in the denominator. A pass requires a measured result, and an
unmeasured workflow earns no credit. Silent wrong results, crashes and data
loss in supported workflows block release even when the score is high.

At `c7fa992f`, 7 of 20 workflows passed on both native Rport and optimized
Wasm, up from 2. The five additions cover missing data, categorical
summaries, local dates across daylight saving time, binomial probability
prediction and rank-deficient models. The optimized Wasm artifact passed all
322 public contract executions and all 77 Chromium tests on macOS arm64.
[Seven-workflow receipt](ci-checkpoints/everyday-analysis-v1-c7fa992f.json)

The first run passed 2 of 20 workflows on both native Rport and optimized
Wasm. It verified CSV import, joining, grouping and export, plus linear
model fitting, prediction and RDS persistence. The other 18 workflows had
no measured outcome in that run. The native shards ran at `89d670fe` and the Wasm
module used the same runtime sources with the new fixtures enabled.
[Baseline receipt](ci-checkpoints/everyday-analysis-v1-89d670fe.json)

Use a pinned strict GNU release report or the individual authenticated shards
that contain the workflow fixtures. For Wasm, use the output of the production
contract runner and the same module it executed. The scorecard verifies the
module checksum and requires both session generations to finish their recovery
checks. Save the scorecard with the source checkout and build receipts.
[How to run the gates](runtime-checkpoints.md)

```bash
python3 scripts/everyday_analysis_scorecard.py \
    --native-report "$report_dir/union.json" \
    --wasm-log "$report_dir/wasm-contracts.log" \
    --wasm-module "$report_dir/pkg/r_wasm_bg.wasm" > "$report_dir/everyday-analysis.json"
```

Repeat `--native-report` when using selected shards from the same source and
corpus. The JSON includes the inventory version, the native execution identity,
the Wasm digest and each workflow's outcome.

To verify a CSV export in a fresh session, run
`python3 scripts/everyday_analysis_sessions.py "$report_dir/pkg"`. It compares
the exported file and two independent sessions against pinned GNU R. Save its
JSON output and pass it as `--session-report` to the scorecard.
