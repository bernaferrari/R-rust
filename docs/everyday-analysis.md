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

At `1790c295`, **16 of 20 workflows (80%)** passed on both targets. The
pinned R6 workflow also verified that loading refuses incompatible versions
for namespace imports and package attachment. A failed load left the
target namespace unregistered. A retry succeeded after making R6
available, and a loaded R6 kept its identity when the library path changed.
The test includes an unused Imports entry and a newer R requirement.
[Scorecard](ci-checkpoints/everyday-analysis-v1-1790c295.json)
[R6 receipt](ci-checkpoints/everyday-r6-1790c295.json)

At `0ea69c8a`, **16 of 20 workflows (80%)** passed on native Rport
and production Wasm. A source script rebuilt an HTML report, a CSV table and
a PDF with two fitted lines after one source measurement changed. Both
targets matched the HTML and CSV produced by pinned GNU R in each run.
Independent figure checks verified the eight observations, the fitted
lines and the labels. The second run started in a fresh session with the
previous artifacts present.
[Scorecard](ci-checkpoints/everyday-analysis-v1-0ea69c8a.json)
[Report receipt](ci-checkpoints/everyday-report-0ea69c8a.json)

At `3e5bbcbb`, **15 of 20 workflows (75%)** passed on both targets.
An ordinary `pdf()` script exported a labeled scatterplot with visible
data points, axes, a fitted line and a dashed reference line. Independent
Poppler checks compared the figure to pinned GNU R. A second export with
unsupported raster content reported an error and preserved the existing
file byte for byte on both targets.
[Scorecard](ci-checkpoints/everyday-analysis-v1-3e5bbcbb.json)
[Plot receipt](ci-checkpoints/everyday-base-plot-3e5bbcbb.json)

At `6b6dfaab`, **14 of 20 workflows (70%)** passed on native Rport and the
smaller production Wasm build. Fresh sessions repeated the CSV round
trip, nine workspace interchange combinations, and the pinned R6 analysis.
[Scorecard](ci-checkpoints/everyday-analysis-v1-6b6dfaab.json)
[Production Wasm receipt](ci-checkpoints/wasm-bundle-6b6dfaab.json)

At `87328a0c`, **14 of 20 workflows (70%)** passed on both native Rport and
production Wasm. An ordinary multi-file project now copies its nested source
tree, reads sibling CSV input through `source(chdir = TRUE)`, writes the
same grouped report as pinned GNU R, and restores the caller’s working
directory after both success and error. The workflow also verifies merge
conflicts, output cleanup and unchanged inputs.
[Scorecard](ci-checkpoints/everyday-analysis-v1-87328a0c.json)
[Production Wasm receipt](ci-checkpoints/wasm-bundle-87328a0c.json)

At `bc598c83`, the smaller production Wasm package and native Rport both
passed **13 of 20 workflows (65%)**. All 12 selected native GNU conformance
cases passed. The package repeated the CSV round trip, all nine workspace
interchange combinations, and the pinned R6 analysis.
[Scorecard](ci-checkpoints/everyday-analysis-v1-bc598c83.json)
[Production Wasm receipt](ci-checkpoints/wasm-bundle-bc598c83.json)

At `a0d25066`, **13 of 20 workflows (65%)** passed on both native Rport and
optimized Wasm. The pinned R6 2.6.1 package constructed an analysis object
and returned the same result on GNU R, native Rport, and Wasm. The browser
session loads explicitly supplied R sources from its private package library.
The check also covers unavailable packages, incomplete metadata, version
constraints, native compilation refusal, and session isolation.
[Scorecard](ci-checkpoints/everyday-analysis-v1-a0d25066.json)
[R6 receipt](ci-checkpoints/everyday-r6-a0d25066.json)

At `56a3fe41`, **12 of 20 workflows (60%)** passed on both native Rport and
optimized Wasm. The workspace workflow saved a fitted model and its source
data from GNU R, native Rport and production Wasm. Fresh sessions on all three
engines read every file and returned the same predictions. The module also
completed all 328 production contract executions and recovery checks.
[Scorecard](ci-checkpoints/everyday-analysis-v1-56a3fe41.json)
[Workspace interchange receipt](ci-checkpoints/everyday-workspace-56a3fe41.json)

At `169031b1`, **11 of 20 workflows (55%)** passed on native Rport and
optimized Wasm. The new checks cover seasonal forecasting, seeded resampling,
gzip tabular import, and reloading exported analysis data in a separate
session. The CSV exported by each target matched the GNU file byte for byte.
The optimized runtime completed all 328 contract executions and recovery
checks. [Scorecard](ci-checkpoints/everyday-analysis-v1-169031b1.json)
[Fresh-session file receipt](ci-checkpoints/everyday-file-round-trip-169031b1.json)

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

Set `--workflow fresh-session-restore` to run the nine GNU, native and Wasm
workspace combinations. Pass that JSON as a second `--session-report`.

Set `--workflow pure-r-package` to install the pinned R6 source on GNU R and
verify the same analysis through native Rport and the production Wasm package.
Pass its JSON as a third `--session-report`.

Set `--workflow base-plot-export` to check a real PDF against pinned GNU R
using `pdfinfo`, `pdftotext`, and `pdftoppm` from Poppler. Pass the JSON
output as another `--session-report`.

Set `--workflow repeatable-report` to build the HTML report, forecast
table, and PDF before and after changing the input CSV. The rendered figure
checks use the same Poppler tools. Pass the resulting JSON as another
`--session-report`.

## Project files in Wasm

R code resolves absolute paths inside its own virtual filesystem. Relative
paths use the session’s working directory. Host imports, exports and file
lists use validated keys relative to the session root. Directory copies,
metadata and connection writes stay inside the session. A quota refusal
during a copy leaves the previous destination intact. Browser paths do not
create links, and file modes are stored as virtual metadata. New native
files copied by `file.copy()` require a filesystem supporting hard links
for exclusive publication. Native directory copies reject destination
symlinks.

## PDF plots

The shipped native CLI and Wasm support file-backed base graphics through
`pdf()` and `dev.off()`. The device writes searchable vector PDF 1.4 in
RGB with bundled DejaVu Sans faces. It accepts literal filenames and
`onefile = TRUE`. Device width and height define the page, and `compress`
controls compression of the page drawing streams. The file is published
after a successful close. A failed export preserves an existing destination.

The device rejects raster images, unsupported glyphs, additional font
families, and alternate paper and color models. It accepts up to 256 pages,
subject to the session memory budget. Wasm PDF files also use the virtual
filesystem limits of 1 MiB per file and 8 MiB per session. PDF options for
title metadata and printer-specific layout settings are outside the verified
support boundary.
