expected_imports <- list(base = TRUE, graphics = c(polypath = "polypath", sunflowerplot = "sunflowerplot", 
plot.new = "plot.new", layout.show = "layout.show", matplot = "matplot", 
boxplot = "boxplot", panel.smooth = "panel.smooth", box = "box", 
lcm = "lcm", locator = "locator", plot.design = "plot.design", 
barplot.default = "barplot.default", axis.POSIXct = "axis.POSIXct", 
curve = "curve", matpoints = "matpoints", plot.default = "plot.default", 
pairs.default = "pairs.default", coplot = "coplot", par = "par", 
co.intervals = "co.intervals", hist.default = "hist.default", 
xyinch = "xyinch", mtext = "mtext", frame = "frame", screen = "screen", 
image.default = "image.default", boxplot.default = "boxplot.default", 
stem = "stem", polygon = "polygon", stars = "stars", lines = "lines", 
symbols = "symbols", pairs = "pairs", points = "points", grconvertX = "grconvertX", 
assocplot = "assocplot", spineplot = "spineplot", grconvertY = "grconvertY", 
title = "title", barplot = "barplot", persp = "persp", legend = "legend", 
identify = "identify", image = "image", filled.contour = "filled.contour", 
lines.default = "lines.default", hist = "hist", stripchart = "stripchart", 
contour = "contour", plot.function = "plot.function", mosaicplot = "mosaicplot", 
fourfoldplot = "fourfoldplot", split.screen = "split.screen", 
smoothScatter = "smoothScatter", matlines = "matlines", axTicks = "axTicks", 
arrows = "arrows", text.default = "text.default", points.default = "points.default", 
xinch = "xinch", dotchart = "dotchart", strheight = "strheight", 
rect = "rect", grid = "grid", .filled.contour = ".filled.contour", 
close.screen = "close.screen", axis = "axis", layout = "layout", 
contour.default = "contour.default", cdplot = "cdplot", pie = "pie", 
xspline = "xspline", text = "text", strwidth = "strwidth", axis.Date = "axis.Date", 
boxplot.matrix = "boxplot.matrix", bxp = "bxp", abline = "abline", 
plot.xy = "plot.xy", rasterImage = "rasterImage", segments = "segments", 
yinch = "yinch", plot = "plot", plot.window = "plot.window", 
clip = "clip", erase.screen = "erase.screen", rug = "rug", Axis = "Axis"
), grDevices = c(as.graphicsAnnot = "as.graphicsAnnot", dev.cur = "dev.cur", 
dev.flush = "dev.flush", dev.hold = "dev.hold", dev.interactive = "dev.interactive", 
dev.new = "dev.new", dev.set = "dev.set", devAskNewPage = "devAskNewPage", 
extendrange = "extendrange", n2mfrow = "n2mfrow", palette = "palette", 
xy.coords = "xy.coords"), utils = c(count.fields = "count.fields", 
flush.console = "flush.console", modifyList = "modifyList", str = "str", 
head = "head", tail = "tail", .checkHT = ".checkHT"))

ns <- asNamespace("stats")
f <- stats::is.ts
stopifnot(identical(environment(f), ns), length(getNamespaceExports(ns)) == 465L)
imports <- getNamespaceImports(ns)
stopifnot(identical(names(imports), names(expected_imports)), is.null(getNamespaceImports("base")))
for (package in names(expected_imports)) {
    actual <- imports[[package]]
    expected <- expected_imports[[package]]
    if (isTRUE(expected)) stopifnot(identical(actual, expected))
    else stopifnot(identical(actual[order(names(actual))], expected[order(names(expected))]))
}
# The embedded original image preserves the captured import ordering as well.
if (length(.libPaths()) == 0L) stopifnot(identical(imports, expected_imports))
stopifnot(identical(get("hist", envir = parent.env(ns), inherits = FALSE), graphics::hist))
stopifnot(identical(environment(graphics::hist), asNamespace("graphics")))
stopifnot(identical(environment(utils::str), asNamespace("utils")))
stopifnot(identical(environment(methods::is), asNamespace("methods")))
x <- stats::ts(1:3)
stopifnot(stats::is.ts(x), identical(stats::tsp(x), c(1, 3, 1)))
invisible(utils::str(quote(a + b)))
detach("package:stats")
stopifnot(identical(asNamespace("stats"), ns))
library(stats)
stopifnot(identical(get("is.ts", envir = as.environment("package:stats")), f))
stopifnot(!exists("is.ts", globalenv(), inherits = FALSE))
TRUE
