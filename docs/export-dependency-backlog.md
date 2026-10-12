# base-package export dependency backlog (demos.R driver)

Generated 2026-10-12 by running `ls(as.environment("package:base"),
all.names=TRUE)` on both the pinned oracle Rscript
(bac583951b728e97b9786804d3b4081f0fe18df5; 1415 names) and the rport engine
(`/tmp/rust_runner`; 791 names), then classifying every missing name from the
upstream definition graph (r-source/src/library/base/R, profile/Common.R,
src/main/names.c) plus a ripgrep audit of crates/. Row-level data:
`export-dependency-backlog.tsv`.

Gap this snapshot: 765 names (the ~638 figure quoted earlier has already shrunk
to a net 624 as neighboring work landed; the 141 names we export that the
oracle lacks are out of scope here).

## Category counts (sum = 765; no 'unknown' rows)

| category | meaning | count |
|---|---|---|
| a | upstream R closure, loadable unchanged | 738 |
| b | alias/replacement sharing an implementation | 13 |
| c | exists in our tree but not bound/registered in package:base | 1 |
| d | blocked on one missing primitive/native (or native-symbol registration) | 11 |
| e | platform/startup-specific binding written by C, not in base/R | 2 |
| f | genuinely missing algorithm (no upstream definition to load) | 0 |
| | **total** | **765** |

Roughly 751 of 765 missing names (98%) are plain
upstream base/R code: the bulk of the backlog is loading the vendored R files,
not writing new algorithms. Only the d/e rows need engine work first.

## Top dependency clusters (upstream file = one load unit)

Each cluster counts the missing names that loading/owning that unit unblocks.

| # | cluster (upstream file) | names unblocked |
|---|---|---|
| 1 | datetime.R | 82 |
| 2 | New-Internal.R | 56 |
| 3 | connections.R | 48 |
| 4 | dates.R | 35 |
| 5 | files.R | 34 |
| 6 | conditions.R | 30 |
| 7 | namespace.R | 25 |
| 8 | version.R | 20 |
| 9 | factor.R | 17 |
| 10 | octhex.R | 16 |

Thematic (cross-file) clusters, largest first:

- **Date/POSIXt/difftime family (datetime.R + dates.R)** — 117 names. needs strptime/POSIXct internals already present; largest single cluster.
- **connections family (connections.R + files.R subsets)** — 48 names. needs the file/table connection internals (file, pipe, url, gzfile, ...).
- **filesystem wrappers (files.R)** — 34 names. list.files/file.path/file.show etc. over platform file natives.
- **condition/restart machinery (conditions.R)** — 30 names. needs .Internal(.signalCondition/.invokeRestart/.addCondHands/.getRestart) verified.
- **namespace machinery (namespace.R)** — 25 names. needs .Internal(importIntoEnv/unregisterNamespace/wrap_meta) verified.
- **numeric_version/package_version family (version.R)** — 20 names. self-contained S4/S3 cluster; getRversion, [.numeric_version, ....
- **factor utilities (factor.R)** — 17 names. reorder, xtabs-adjacent factor ops; depends only on available primitives.
- **hexmode/octmode (octhex.R)** — 16 names. fully self-contained arithmetic-method cluster.
- **dyn.load/DLLInfo family (dynload.R)** — 16 names. needs .C_R_* native routine objects registered (see d rows).
- **data.frame method family (dataframe.R)** — 16 names. [.data.frame/[[.data.frame SCC; mutually recursive but closed.

Cross-cutting internal gaps (names whose upstream body calls an
`.Internal` we could not find in crates/): .signalCondition (4),
importIntoEnv (3), .invokeRestart (3), .getRestart / unregisterNamespace /
qr_qy_cmplx / qr_qy_real / .addCondHands (2 each), plus single users
(wrap_meta, setEncoding, compilerVersion, Sys.glob, tzone_name, copyDFattr,
activeBindingFunction, objsxp, mkUnbound, vector, ...) — each is a
proof obligation on the corresponding a-row, not a category of its own.

## Quick wins the analysis surfaced

1. **Chained-assignment aliases (13 b-rows)**: `double`<->`numeric`,
   `license`<->`licence`, `duplicated/anyDuplicated/unique` `.matrix`<->`.array`,
   `range.POSIXct`<->`range.Date`, `[.Dlist`<-`[.simple.list`<-`[.listof` —
   zero new code once the chain's first target loads; just keep the chain
   when transcribing upstream files.
2. **`.Last.value` (c-row)**: referenced 23x in our evaluator but never bound
   in package:base; one binding at toplevel-eval time.
3. **`.First.sys` / `.OptRequireMethods`** live in profile/Common.R, not
   base/R — remember to load that file too or demos' autoload machinery stalls.
4. **Compose/Funcall/Partial/.enc2utf8_sub/.OBJSXP** exist in the oracle base
   image but NOT in the vendored r-source tree (tree is slightly older than
   the oracle build): recover definitions via `deparse(get(name, baseenv()))`
   on the oracle.
5. **.F_* / .C_R_* routine objects (10 d-rows)**: base's own DLL symbols
   (dqrdc2/dqrcf/... and task-callback .Call entries) surface as bindings
   `.F_dqrcf` etc. in the namespace; need native-symbol registration, not R code.

## Method / reproducibility

oracle list: `Rscript -e 'writeLines(sort(ls(as.environment("package:base"), all.names=TRUE)))'`;
ours: same script through `/tmp/rust_runner`; `LC_ALL=C comm -23` gives the
missing set. Classification: parse top-level assignments (incl. chained and
backticked targets) across r-source/src/library/base/R/**.R and
profile/Common.R, build the missing-name dependency graph (SCC fixpoint for
mutual recursion, e.g. [.data.frame <-> [[.data.frame), check
src/main/names.c for primitives, and ripgrep crates/ for natives/internals
already implemented. Names loadable after the fixpoint are category a;
the residual leaves are d/e/f as defined above.
