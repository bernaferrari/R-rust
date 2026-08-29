# src/library Sync Window Dispositions (HEAD..origin/trunk)

Scope: every `src/library` file changed between r-source `HEAD` (d4cc5d9) and
`origin/trunk`, excluding `/man/` and `/tests/`, dispositioned against the Rust
port (rmath-rs). Dispositions:

- **applicable (ported)** — the port implements the same surface; the trunk
  change was ported (or was already present).
- **known-gap** — the port implements (or plans to implement) the surface and
  the trunk change is not yet mirrored; a follow-up porting item.
- **N-A** — R-level/C-level code with no ported counterpart; the trunk change
  has no port surface to sync.

Gate reference: `scripts/upstream_core_slices.sh` (upstream slices),
`scripts/conformance_parity.sh` (conformance parity).

## base/R

| File | Trunk change (window) | Ported surface? | Disposition |
| --- | --- | --- | --- |
| structure.R | `.Deprecated()` warning listing renamed special names (`.Dim`/`.Dimnames`/`.Names`/`.Tsp`/`.Label` → `dim`/`dimnames`/`names`/`tsp`/`levels`) | `do_structure` (mainutils/essentials/matrix.rs) performs the same rename | **applicable — ported**: rename table + one `warningcall` per call with the sQuote'd name lists; deferred-warning print now renders `In <deparse1s(call)> :` via the errors.c-faithful `PrintWarnings` (see `errors.rs::take_warnings_block`). Byte-verified against trunk. Pre-window note: `structure(NULL, ...)` must error ("attempt to set an attribute on NULL"); the port still returns NULL silently — flagged, not part of this window. |
| RNG.R | new `binom.kind` argument (Buggy BTPE / BTPE / default) | `RNGkind` builtin (eval/builtin.rs, sexp/instance.rs) | **known-gap** — binomial-kind selection not ported (feature, not bugfix). |
| load.R, example.R (utils) | call `RNGkind` with the 4th kind argument | port's RNGkind/example keep 3-kind signature | **N-A until RNG.R gap closes** (internally consistent). |
| aperm.R | `aperm.default` NULLs the class when `keep.class = FALSE` | `do_aperm` (mainutils/array.rs) has no `keep.class` argument (perm/resize only); the PR#19133 matrix-transpose and PR#19069 identity special cases are already ported | **known-gap** — add `keep.class` argument handling. |
| dataframe.R | `anyDuplicated(nm)` refactor of row-name dedup | data.frame support lives in builtins; this R file not ported | **N-A** (behavior-preserving refactor). |
| datetime.R | tz discovery via `timedatectl show --property=Timezone --value` | port TZ handling (tzone.rs) has no timedatectl path | **N-A**. |
| dcf.R | `.enc2utf8_sub` canonicalization of DCF fields (invalid UTF-8 → `<xx>` escapes) | `read.dcf` Rust port (mainutils/dcf.rs) | **known-gap** — port does not canonicalize non-UTF-8 DCF input. |
| funprog.R | new `Compose()` and `Funcall()` functions | port's functional builtins (Reduce/Filter/Map/…) have no Compose/Funcall | **known-gap** — new base API. |
| library.R | `require()` gains `pos` argument | `do_require` (essentials) is a simplified port without `pos` | **known-gap** (minor). |
| merge.R | empty-result `cbind(x[FALSE, , drop=FALSE], …)` fix | no merge surface in port | **N-A**. |
| methodsSupport.R | new `.OBJSXP() <- .Internal(objsxp())` binding | `objsxp` internal already ported (objects.rs, registered in eval/builtin.rs); no ported R code references `.OBJSXP` | **N-A** (internal exists; R wrapper binding unreferenced by ported surfaces). |
| namespace.R | export-resolution perf rework (`mget`/`ifnotfound`, vapply) | loadNamespace/namespace R-level not ported (`::`/`:::` builtins only) | **N-A**. |
| rm.R | `.Primitive("c")(list, …)` → `c(list, …)` | `rm` builtin; R file not ported | **N-A** (behavior-preserving). |

## grDevices

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| grDevices-defunct.R | xfig `.Defunct` message restructure | not ported | **N-A**. |
| postscript.R, unix/dev2bitmap.R, windows/dev2bitmap.R | "GhostScript was not found" → "Ghostscript" spelling | port's postscript surface has no Ghostscript check | **N-A**. |
| src/cairo/cairoFns.c | alpha-0 draw skip also when `xd->appending` | no cairo device port | **N-A**. |
| src/devPS.c | `papersize` option robustness (non-string/empty) + `safestrcpy` | port devps.rs has no papersize option path | **N-A**. |

## graphics

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| boxplot.R | `split(c(x), col(x)/row(x))` refactor | no boxplot in port | **N-A**. |
| filled.contour.R | `col` length mismatch warning | port has `C_filledcontour` (library/graphics/plot3d.rs) but not the R wrapper that validates `col` | **N-A** (wrapper unported; warning would ride along if the wrapper is ever ported). |

## grid

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| src/gpar.c | `setFontFamily` clamps family to `char[201]` | port grid `initGContext` validates gpar without copying into fixed C buffers (gpar.rs) | **N-A** (no fixed-buffer copy exists to overflow). |
| src/layout.c | `allocationRemaining`: `initial == 0` now returns FALSE | port layout.rs `allocationRemaining` already returns `false` for `initial == 0` | **applicable — already aligned**. |

## methods

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| ClassExtensions.R | setIs S3-part extraction reorder | R-level; not ported | **N-A**. |
| MethodsList.R | removes defunct MethodsList helpers | never ported | **N-A**. |
| addedFunctions.R | `.Primitive("[[<-")` → `` `[[<-` `` | not ported | **N-A** (equivalent). |
| makeBasicFunsList.R | rcond implicit-table setMethod | not ported | **N-A**. |
| methodsTable.R | generic check against `"genericFunction"` | not ported | **N-A**. |
| oldClass.R | whitespace only | — | **N-A**. |
| trace.R | `.class1()` instead of `class(x)[1L]` | no trace surface in port | **N-A**. |
| src/methods_list_dispatch.c | dead static/symbol cleanup only | port mirrors the dispatch logic (library/methods/methods_list_dispatch.rs) | **N-A** (behavior unchanged upstream). |

## parallel

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| mvrnorm0.R (new) | unexported MASS-free test helper | port parallel module (fork/rngstream/ncpus) doesn't port it | **N-A**. |
| DESCRIPTION.in | build metadata | — | **N-A**. |

## stats

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| add.R | binomial matrix response with `y=FALSE` (PR#19128) | add.term R-level not ported | **N-A**. |
| aggregate.R | `nlevels(y)` refactor | `do_aggregate` (tables.rs) is a simplified numeric-by-groups builtin; trunk change is behavior-preserving | **N-A**. |
| aov.R | multi-response perfect-fit offset fix | not ported | **N-A**. |
| binom.test.R, fisher.test.R, mantelhaen.test.R, poisson.test.R | new `two.sided.method` argument ("minlike"/"central") | test functions not ported | **N-A** (feature; would be known-gap if the surfaces are ported later). |
| dummy.coef.R | term names via `deparse1` of variables attr | not ported | **N-A**. |
| free1way.R | tryCatch around Matrix::solve | not ported (requires Matrix) | **N-A**. |
| glm.R | model.frame method logic (PR#19036) | not ported | **N-A**. |
| lm.R | zero-residual variance guard for length-1 fits | not ported | **N-A**. |
| mlm.R | `Rank(X) >= Rank(M)` identifiability guards | not ported | **N-A**. |
| nls.R | upper/lower bound normalization rework | not ported | **N-A**. |
| wilcox.test.R | comment-only (z → w) | — | **N-A**. |
| src/cov.c + src/cov_kendall.c | Kendalltau helper extracted behind `kendall_wrapper` | port already modular: library/stats/kendall.rs | **N-A — already aligned**. |
| src/portsrc.f | `TEMP1 = IV(STLSTG)` fix before DL7TVM/DL7VML | port optim.rs does not port the Fortran PORT routines | **known-gap** — PORT optimizer not ported; fix rides along when it is. |

## tcltk

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| src/tcltk_unix.c | commented-out dead statics | port tcltk_unix.rs stub mirrors live behavior only | **N-A**. |

## tools

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| R/* (CRANtools, QC, Rd2HTML, Rd2pdf, Rd2txt, RdHelpers, admin, apitools, bibtools, build, check, htmltools, mailtools, packages, sotools, testing, toHTML, utils) | package-maintenance tooling (new checks, mergeImportFroms perf, grammar-table consumers, message fixes) | port tools module implements C internals only (getfmts/gramrd/gramlatex/install/md5/sha256); none of these R files are ported | **N-A**. |
| NAMESPACE | exports | — | **N-A**. |
| src/getfmts.c | `Rf_strchr_const` const-correctness | port getfmts.rs: behavior unchanged | **N-A**. |
| src/gramLatex.c/.y, src/gramRd.c/.y | regenerated bison tables (grammar extension) | port gramlatex.rs/gramrd.rs are hand-ports of the generated parsers | **known-gap** — port parser tables lag the new grammar until the upstream grammar features are ported. |
| src/init.c | `CALLDEF(codeFilesAppend, 3)` arity | codeFilesAppend is not registered in the port (internal utility only) | **N-A**. |
| src/install.c | `codeFilesAppend(f1, f2, enc)`: `enc` validation + UTF-8 BOM skip while collating | port `codeFilesAppend` (library/tools/install.rs) | **applicable — ported**: signature + `enc` validation + BOM consume/rewind synced byte-for-byte with install.c. |
| src/tools.h | declaration updates | — | **N-A**. |

## utils

| File | Trunk change | Ported surface? | Disposition |
| --- | --- | --- | --- |
| NAMESPACE | exports | — | **N-A**. |
| SweaveDrivers.R | `strip.white` logical-valued options | not ported | **N-A**. |
| aspell.R | Sweave filter wrapper (latex passthrough) | not ported | **N-A**. |
| citation.R | MARC relator `term` lookup | not ported | **N-A**. |
| data.R | `zst`/`zstd` compressed-data suffixes | data() not ported | **known-gap** (compression-suffix list when data() is ported). |
| example.R | RNGkind 4-argument save/restore | `example` builtin uses port RNGkind (3-kind) | **N-A until RNG.R gap closes** (internally consistent). |
| package.skeleton.R | default `encoding = "UTF-8"` | not ported | **N-A**. |
| str.R | `startsWith(cl, mod)` refactor | `str` builtin ported; trunk change is behavior-preserving | **N-A**. |

## Synced in this window (summary)

1. `structure()` special-name deprecation warning — construct side of the
   dotted-attribute remap (pairs with the SyncErrDep deparse-side work); the
   deferred-warning printer (`PrintWarnings`) now matches errors.c including
   the `In <deparse1s(call)> :` header and LONGWARN wrapping.
2. `codeFilesAppend` — `enc` argument + UTF-8 BOM skip.
3. Confirmed already aligned: grid `allocationRemaining`, kendall extraction,
   aperm PR#19133/PR#19069 special cases.
