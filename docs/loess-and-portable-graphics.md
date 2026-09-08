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

The portable renderer now shares session-owned plot coordinates across `plot`,
`lines`, `points`, `segments`, `arrows`, `abline`, `rect`, `polygon`, `text`,
`title`, `axis`, `box`, `plot.new` and `plot.window`. It supports ordinary plot
types, logarithmic/reversed limits, finite-data filtering, uniform `mfrow`
panels, color names/palettes, line styles, point symbols, clipping and rotated
text. A licensed, bundled Noto Sans font supplies text on Wasm without filesystem
access. Host fonts and explicitly supplied fonts can still override it.

## Evidence and limits

The numerical regression module compares fits, residual diagnostics, robust
weights and standard errors against the pinned GNU R oracle. The generator
scripts in `tests/loess` reproduce the multivariate and weighted reference
values. Conformance cases 570 and 571 exercise the public R interface and
omission/NA behavior. Public rendering tests inspect PNG colors and labels, layer a fitted LOESS curve, exercise custom
S3 methods and verify clipping and bundled-font glyphs.

This is not complete GNU R graphics compatibility. Portable primitives do not
establish complete base/grid/ggplot rendering, device/display-list replay,
plotmath, patterns/masks/groups, or every graphical parameter. Automatic linear
ticks follow GNU R's GEPretty spacing and `par("lab")`,
including reversed axes; automatic axes reject requests above 10,000 intervals.
Logarithmic tick selection, margins, some point symbols and logarithmic ablines
still differ from GNU R. High-level plotting functions require separate
contract coverage.

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
