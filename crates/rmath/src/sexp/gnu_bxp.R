{
bxp <- function(z, notch = FALSE, width = NULL, varwidth = FALSE,
                outline = TRUE, notch.frac = 0.5, warnN = TRUE,
                log = "", border = par("fg"),
                pars = NULL, frame.plot = axes, horizontal = FALSE,
                ann = TRUE, add = FALSE, at = NULL, show.names = NULL,
                panel.first = NULL, panel.last = NULL, ...)
{
    if (...length()) {
        nmsA <- names(args <- list(...))
        if (anyDuplicated(nmsA)) {
            iD <- duplicated(nmsA)
            warning(sprintf(ngettext(sum(iD),
                                     "Duplicated argument %s is disregarded",
                                     "Duplicated arguments %s are disregarded"),
                            sub("^list\\((.*)\\)", "\\1", deparse(args[iD]))),
                    domain = NA)
        }
    }
    invisible(z)
}
boxplot <- function(x, ..., plot = TRUE) {
    list(stats = matrix(0, 5, 1), n = length(x), conf = NULL, out = numeric(),
         group = numeric(), names = "1")
}
bxp
}
