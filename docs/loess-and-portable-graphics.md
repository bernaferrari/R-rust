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
RGBA/PNG readback or a GPU texture for host compositing. `RSession::record_scene` finishes R evaluation synchronously;
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
text. A licensed, bundled DejaVu Sans font supplies the same text and mathematical
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
segments, polygons, text and circular points; the corresponding `*Grob`
constructors; `gList`, `gTree`, `grobTree` and `grid.draw`. Grob trees inherit
`gpar` through their viewport, and viewport scopes unwind when child drawing
fails. Text supports the shared plotmath decoder. Viewports compose translation,
rotation, sizing and native axis scales, with push/pop stacks owned by the R
session. Axis-aligned clipping and equal/weighted/absolute grid layouts work.
Drawing commands feed the same owned scene used by CPU/GPU devices and
`recordPlot`/`replayPlot`.

`unit` and `convertX`, `convertY`, `convertWidth`, `convertHeight` support npc,
snpc, native, inches, centimetres, millimetres, points, big points, picas, dida,
cicero, scaled points, lines and char units. Layout null units share remaining
space after absolute dimensions. Numeric regression values for physical/native
units and weighted layouts were checked against the pinned GNU R oracle at a
known device size; PNG tests check actual viewport placement and clipping.

This is a bounded grid frontend, not the complete GNU R grid package. Unit
arithmetic and data-dependent units, named viewport navigation, gPath editing,
layout respect, rotated clipping, arrows, compound polygon groups, non-solid
line types, text overlap checking and non-circular point symbols remain gaps.
Unsupported drawing parameters fail explicitly. Text-dependent char/line units
currently use device font-size conventions, not GNU R font metric parity.
Recordings preserve drawing commands; restoring a live grid viewport stack
from a recording is not implemented. Full ggplot2 compatibility is not claimed.

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

Supported constructions include Greek names, fractions (`frac`, `over`),
stacked expressions without a rule (`atop`), subscripts and superscripts,
square roots, fixed delimiters (`group` and parentheses), concatenation and
spacing (`paste`, `*`, `~`), phantom contents, arithmetic/comparison operators,
and common function names. `sum`, `prod`, and `integral` accept a body and optional
lower/upper limits placed as scripts. Ordinary `hat`, `tilde`, `dot`, `ring`,
`bar`, and `underline` accents are also available. `plain`, `bold`, `italic`, and
`bolditalic` explicitly select the shared renderer's font face; bold and italic
are synthetic treatments of the same outlines. Latin variable names default to italic; explicit face wrappers override that convention. Greek symbols and numeric constants remain upright, as specified by [R mathematical annotation](https://stat.ethz.ch/R-manual/R-devel/library/grDevices/html/plotmath.html).

This is a bounded plotmath implementation, not an exact port of GNU R's
font-specific mathematical typography. It does not yet implement stretchy
`bgroup` delimiters, wide accents,
display-style centered operator limits, or every plotmath symbol/operator.
Unsupported operators report errors. The decoder rejects trees deeper than 64
levels or exceeding 4096 decoded nodes per expression. Public tests verify
Greek glyph selection, independently positioned scripts, fraction/radical
geometry, actual rendered pixels, style propagation, title/axis integration,
and record/replay; they do not establish pixel equivalence with GNU R devices.

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
Logarithmic tick selection, margins and logarithmic ablines
still differ from GNU R. High-level plotting functions require separate
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
