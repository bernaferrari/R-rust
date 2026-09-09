# LOESS and portable graphics

The public `loess`, `loess.control` and `predict.loess` functions now use an
owned Rust numerical engine. The numerical module forbids unsafe code and uses
faer SVD with the default backend. The optional system-backend build uses a
local safe Jacobi SVD for LOESS; system LAPACK does not provide R's LOESS routines.

The implementation includes local degree 0–2 polynomial fits, one through four
predictors, tricube neighborhoods, normalization, observation weights,
parametric predictors, dropped squares, robust iterations, direct prediction,
KD-cell Hermite interpolation, exact/approximate diagnostics and prediction
standard errors. The R adapter supports additive formulas, list/data-frame data,
matrix predictors, subsets and optional model frames. Classed `na.omit` and
`na.exclude` metadata survives serialization; `predict(fit)` restores excluded
rows, while explicit `newdata` and `se=TRUE` retain GNU R's distinct behavior.
Unavailable predictions preserve R's NA sentinel rather than computational NaN. Fitted objects contain
ordinary R values and can cross serialization boundaries without Rust pointers.

```r
x <- seq(0, 1, length.out=30)
y <- sin(5*x) + cos(31*x)/8
fit <- loess(y ~ x)
plot(x, y, pch=21, bg="gold", main="LOESS μ")
lines(x, predict(fit), col="red", lwd=3)
```

The PNG backend uses [Vello CPU](https://github.com/linebender/vello), with vector
paths and glyph outlines, transformed RGBA images, alpha compositing and clipping.
The synchronous host API works without a GPU on native and Wasm. The optional
`r-device-vello-gpu` crate uses Vello 0.10 and wgpu 29 compute pipelines, with
explicit asynchronous initialization, adapter information, texture rendering and
RGBA/PNG readback or a GPU texture for host compositing. The browser API can attach, resize and present directly to an HTML canvas. Native Rust hosts can pass a safe wgpu window target into `new_for_surface` and use the same intermediate-texture/blit strategy. Android/Swift UI-layer surface integration still requires a host implementation. `RSession::record_scene` finishes R evaluation synchronously;
the resulting owned scene can outlive the session and be rendered asynchronously.
Enable `r-embed/vello-gpu` for its `GpuRenderer` re-export. See [GPU API and tests](../crates/r-device-vello-gpu/README.md) for native and
browser usage. GPU initialization
errors are returned to the caller; no implicit CPU fallback is performed. Canvas admission rejects zero dimensions,
dimensions above 65,535, and more than 16,777,216 pixels.

The portable renderer now shares session-owned plot coordinates across `plot`,
`lines`, `points`, `segments`, `arrows`, `abline`, `rect`, `polygon`, `text`,
`title`, `axis`, `box`, `plot.new` and `plot.window`. It supports ordinary plot
types, logarithmic/reversed limits, finite-data filtering, uniform `mfrow`
panels, color names/palettes, line styles, pch 0–25 and character symbols, xpd clipping and rotated
text. The licensed, bundled DejaVu Sans font family supplies the same text and mathematical
glyphs on native and Wasm without filesystem access. Explicit custom font bytes
can override the CPU renderer font.

The additional numeric `hist`, vector/matrix `barplot`, and numeric/list `boxplot`
methods return statistical objects as well as drawing through the portable
primitives. Explicit unsupported options report errors, including hatch fills,
legends, notch/variable-width boxes and formula methods. These are bounded
implementations, not complete upstream methods. Automatic histogram breaks use
the public `pretty` frontend over R's existing kernel; requests or generated
outputs above one million intervals are rejected.

`rasterImage` decodes color/grayscale matrices and packed native rasters to owned
RGBA bytes. The device applies affine placement, clipping and optional image
interpolation. `recordPlot` snapshots owned device commands into an R raw object;
`replayPlot` supports serialization roundtrips and resized replay, restoring the
portable user-coordinate transform for subsequent overlays. This recording
format is specific to Rport. It does not implement GNU R recording interchange,
package-reload metadata or full replay of the R expressions and graphical state
that produced the plot.

## Portable grid

`library(grid)` and `require(grid)` attach a built-in grid package; `grid::`
resolves the exported portable functions and rejects names outside that surface.
`getNamespace("grid")` and `requireNamespace("grid")` use the same session-owned,
GC-traced namespace cache as other packages. Package names support the ordinary
unquoted syntax and `character.only=TRUE`.

The current drawing surface includes `grid.newpage`, rectangles, circles, lines,
segments, grouped polygons, text and standard point symbols; the corresponding `*Grob`
constructors; `gList`, `gTree`, `grobTree` and `grid.draw`. Grob trees inherit
`gpar` through their viewport, and viewport scopes unwind when child drawing
fails. Text supports the shared plotmath decoder. Viewports compose translation,
rotation, sizing and native axis scales, with push/pop stacks owned by the R
session. Named navigation uses a persistent viewport tree: `upViewport` retains children, `popViewport` removes the popped subtree, and `downViewport`, `seekViewport`, and `vpPath` resolve named paths. Axis-aligned clipping and equal/weighted/absolute grid layouts work, including `respect=TRUE` and selective respect matrices. The owned allocator follows GNU R’s fixed-length, respected-null, then remaining-null allocation order. `unit.c` combines physical/null lengths; layouts support numeric and named justification, spans, zero/negative null lengths, and centered oversized fixed layouts. Device-space cell bounds are checked against the pinned GNU R oracle in `tests/grid-layout-oracle.R`. Line dashes and arrowheads are supported.
Drawing commands feed the same owned scene used by CPU/GPU devices and
`recordPlot`/`replayPlot`.

`unit` and `convertX`, `convertY`, `convertWidth`, `convertHeight` support npc,
snpc, native, inches, centimetres, millimetres, points, big points, picas, dida,
cicero, scaled points, lines, char, strwidth and strheight units. String dimensions use the shared font metrics. Mixed-dimension unit arithmetic is represented by deferred expression trees. Numeric scaling, unary signs and sum/min/max retain unit names and string data and are exercised under GC torture. `grobWidth` and `grobHeight` support the implemented rectangle, circle and text grobs. Missing coefficients propagate through summaries, including `na.rm=TRUE`, as in the pinned GNU R grid oracle. Layout null units share remaining
space after absolute dimensions. Numeric regression values for physical/native
units and weighted layouts were checked against the pinned GNU R oracle at a
known device size; PNG tests check actual viewport placement and clipping.

This is a bounded grid frontend, not the complete GNU R grid package. `gPath`, `getGrob` and `editGrob` support named nested child paths, descendant search and independent edits to gp/name/vp. Regex/global matching, geometry edits, custom editDetails methods, display-list grid.edit, and arbitrary grob measurement remain gaps. Rotated viewport clipping follows GNU R by warning and retaining the parent clip. Text overlap checking uses rotated text bounds and the shared font metrics.
Unsupported drawing parameters fail explicitly. Text-dependent char/line units
currently use device font-size conventions, not GNU R font metric parity.
Recordings preserve drawing commands and a validated, renderer-specific snapshot
of the portable grid viewport tree, active transforms, layouts, and graphical
parameters. `replayPlot` restores that state and rescales device-space values
for the target dimensions; malformed metadata is rejected before replay state
is committed. The format is Rport-specific and does not provide GNU R recording
interchange or full ggplot2 compatibility.

## Mathematical labels

`text`, `title`, `axis`, and plot titles/axis captions accept `expression()`
labels. `grid.text` uses the same owned math layout. The R adapter decodes the
expression tree before borrowing the renderer; the graphics engine then measures
shared font advances and emits ordinary positioned glyphs and paths. These
commands remain available to scene recording and GPU replay.

```r
plot.new()
plot.window(xlim=c(0, 1), ylim=c(0, 1))
text(.5, .5, expression(frac(alpha[1]^2, sqrt(beta))), cex=2)
title(main=expression(bold(x) + italic(y)))
```

Supported constructions include Greek names, fractions (`frac`, `over`, `atop`),
subscripts and superscripts, square roots, groups, concatenation, spacing,
phantoms, arithmetic/comparison operators, accents, and display operators.
General function calls such as `f(x)` render their names and arguments; they are
not evaluated. Malformed recognized constructions report errors. The decoder
rejects trees deeper than 64 levels or exceeding 4096 nodes per expression.

Plain, bold, oblique, and bold-oblique DejaVu Sans 2.37 faces are bundled.
`plain`, `bold`, `italic`, and `bolditalic` select actual face-specific outlines
and advances. Like GNU R, math starts in the plain face; variables become
italic only when the context requests it. Numeric atoms remain plain.

The layout follows GNU R's display/text/script/scriptscript size transitions,
quad-based operator spacing, script shifts and italic corrections, fraction
clearances, radical geometry, extensible delimiter glyph pieces, and separate
sum versus integral limit placement. A test-only GNU R device measures the same
bundled fonts with FreeType, using point coordinates and a 1/72-inch device
scale. Symbol codes map to Unicode rather than unavailable private-use glyphs;
missing glyphs fail the oracle instead of silently measuring a replacement.

The checked-in oracle covers six decoded expressions—Greek alpha, a nested
fraction, a radical, scalable parentheses, a sum and an integral—at 6, 12 and
24 points. All 18 width/height pairs are compared through the R parser and
owned decoder with a 0.00002-point tolerance. Regenerate and check them with
FreeType, pkg-config, a C compiler, and the pinned GNU R executable available:

```sh
R_BIN=/path/to/pinned/R bash scripts/plotmath_font_oracle.sh
cargo test -p rmath --features renderplot-device --lib decoded_expressions_match_gnu_r_same_font_metrics
```

This establishes same-font metric parity for that corpus, not universal GNU R
pixel equivalence. Rasterization and host font families differ between devices;
accent variants, the complete symbol/style catalog, and arbitrary deeply nested
combinations still need broader oracle coverage. Rendering tests additionally
check actual pixels, explicit faces, title fitting, axes, and record/replay.

## Web showcase

The [Rove website](../website/README.md) includes sixteen editable examples, real
Wasm execution in a worker, PNG export, and a local AI demo using browser WebGPU
models or an optional Ollama endpoint. Browser weights load on explicit request;
generated code remains editable before execution. The page is prerendered and
hydrates into a playground. No sharing feature is enabled.

Browser testing exposed and fixed two runtime issues: random-seed bootstrapping
now uses browser entropy instead of unsupported native time/process APIs, and
`rnorm` and `runif` use the shared portable samplers on Wasm. Seeded `runif`, recycled bounds, degenerate ranges, and RNG consumption are covered by native and actual browser tests. Wasm sessions have a 64 MiB R arena budget, a 500,000-node budget, conservative 1 MiB result-export admission, and a combined 1 MiB console capture limit with an explicit truncation marker. The website runtime is linked with a 256 MiB linear-memory maximum and rejects replacement artifacts without that ceiling. Oversized result graphs are rejected before formatting or host projection; repeated references count toward the export budget. These limits cover the Wasm instance, not browser rendering, GPU resources, or local AI models. Native hosts must configure resource limits and use OS process limits when a total process-memory boundary is required.
Explicit `set.seed` remains reproducible across fresh browser sessions.

## Evidence and limits

The numerical regression module compares fits, residual diagnostics, robust
weights and standard errors against the pinned GNU R oracle. The generator
scripts in `tests/loess` reproduce the multivariate and weighted reference
values. Conformance cases 570 and 571 exercise the public LOESS interface and
omission/NA behavior. Case 572 compares histogram and boxplot statistics and
bar positions; case 573 covers matrix dimension inference exposed by barplots.
Public rendering tests inspect PNG colors and labels, layer a fitted LOESS curve, exercise custom
S3 methods and verify clipping and bundled-font glyphs.

This is not complete GNU R graphics compatibility. Portable primitives do not
establish complete base/grid/ggplot rendering or the GNU R device lifecycle,
complete plotmath typography, patterns/masks/groups, or every graphical parameter. Automatic linear
ticks follow GNU R's GEPretty spacing and `par("lab")`,
including reversed axes; automatic axes reject requests above 10,000 intervals.
Logarithmic `abline` now distinguishes transformed-space straight lines from
the original-coordinate curve requested by `untf=TRUE`, using GNU's 100-interval
sampling contract. Nonfinite coefficients fail recoverably. Logarithmic tick
selection and margins still differ from GNU R. High-level plotting functions require separate
contract coverage. The legacy ArcTo scene command still uses an endpoint line
fallback; general elliptical arcs need an explicit rotation/sweep contract.

LOESS does not yet provide the legacy native `lowes*`/`ehg*` ABI, complete
model-frame/terms metadata, custom NA actions,
`method="model.frame"`, or iteration tracing. Unsupported method/NA/trace
requests report errors. Exact diagnostics still construct dense influence
matrices and have cubic work.
An overflow-checked, conservative workspace estimate rejects operations above
256 MiB before numerical allocation; this is an admission limit for the kernel,
not a limit on total process memory. Fitting, interpolation, prediction and
exact diagnostics poll the host cancellation token. A local SVD finishes before
the next cancellation checkpoint. Large-data performance still needs further
engineering. The fixtures are evidence for covered cases,
not a claim that every singular or degenerate dataset matches R.

## Upstream LOESS notice

Copyright (C) 1998--2020 The R Core Team

The authors of this software are Cleveland, Grosse, and Shyu.
Copyright (c) 1989, 1992 by AT&T.
Permission to use, copy, modify, and distribute this software for any
purpose without fee is hereby granted, provided that this entire notice
is included in all copies of any software which is or includes a copy
or modification of this software and in all copies of the supporting
documentation for such software.
THIS SOFTWARE IS BEING PROVIDED "AS IS", WITHOUT ANY EXPRESS OR IMPLIED
WARRANTY. IN PARTICULAR, NEITHER THE AUTHORS NOR AT&T MAKE ANY
REPRESENTATION OR WARRANTY OF ANY KIND CONCERNING THE MERCHANTABILITY
OF THIS SOFTWARE OR ITS FITNESS FOR ANY PARTICULAR PURPOSE.

## Interactive browser output

The console evaluates each command once and returns captured text plus a PNG
when actual drawing occurs. Assignment-only commands remain invisible, and a
counter regression checks that requesting graphics does not evaluate twice.
One-dimensional numeric arrays and tables are accepted by `barplot`, matching
the pinned oracle's bar centers. Separate console commands still use separate
render targets: persistent cross-command graphics and partial output on errors
remain unfinished.
