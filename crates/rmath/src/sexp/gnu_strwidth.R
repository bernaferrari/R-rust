# Adapted from GNU R src/library/graphics/R/strwidth.R (GPL-2.0-or-later).
function(s, units = "user", cex = NULL, font = NULL, vfont = NULL, ...) {
    .External.graphics("C_strWidth", as.graphicsAnnot(s),
        pmatch(units, c("user", "figure", "inches")), cex, font, vfont, ...)
}
