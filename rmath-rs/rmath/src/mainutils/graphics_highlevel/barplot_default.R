function(height, width = 1, space = NULL, names.arg = NULL, legend.text = NULL,
         beside = FALSE, horiz = FALSE, density = NULL, angle = 45,
         col = NULL, border = "black", main = NULL, sub = NULL,
         xlab = NULL, ylab = NULL, xlim = NULL, ylim = NULL, xpd = TRUE,
         log = "", axes = TRUE, axisnames = TRUE, cex.axis = 1,
         cex.names = 1, inside = TRUE, plot = TRUE, axis.lty = 0,
         offset = 0, add = FALSE, ann = !add, orderH = "none",
         panel.first = NULL, panel.last = NULL, ...) {
    if (!is.null(density)) stop("shading lines in bars are not supported")
    if (!is.null(panel.first) || !is.null(panel.last)) stop("panel hooks are not supported")
    if (!is.null(legend.text)) stop("barplot legends are not supported")
    if (!is.character(log) || length(log) != 1L) stop("'log' must be a character string")
    d <- dim(height)
    vectorInput <- is.null(d)
    if (vectorInput) {
        if (!is.numeric(height)) stop("'height' must be a vector or a matrix")
        original.names <- names(height)
        height <- rbind(height)
        beside <- TRUE
        if (is.null(col)) col <- "grey"
    } else if (length(d) == 2L && is.numeric(height)) {
        original.names <- colnames(height)
        if (is.null(col)) col <- rep("grey", nrow(height))
    } else stop("'height' must be a vector or a matrix")
    NR <- nrow(height)
    NC <- ncol(height)
    if (is.null(space)) space <- if (!vectorInput && beside) c(0, 1) else .2
    space <- space * mean(width)
    if (is.null(names.arg) && axisnames) names.arg <- original.names
    if (beside) {
        if (length(space) == 2L && !vectorInput) space <- rep(c(space[2L], rep(space[1L], NR - 1L)), NC)
        width <- rep(width, length.out = NR)
    } else width <- rep(width, length.out = NC)
    offset <- rep(offset, length.out = length(width))
    if (length(space) == 1L) space <- rep(space, length.out = if (beside) NR * NC else NC)
    delta <- width / 2
    w.r <- cumsum(space + width)
    w.m <- w.r - delta
    w.l <- w.m - delta
    if (nzchar(log) && min(height + offset, na.rm = TRUE) <= 0)
        stop("log scale error: at least one 'height + offset' value <= 0")
    rectbase <- 0
    hdraw <- height
    if (!beside) {
        if (!(orderH %in% c("none", "incr", "decr"))) stop("invalid 'orderH'")
        hdraw <- rbind(rectbase, apply(height, 2L, cumsum))
    }
    rAdj <- offset + if (horiz) 0.9 * hdraw else -0.01 * hdraw
    if (horiz) {
        if (is.null(xlim)) xlim <- range(rAdj, hdraw + offset, na.rm = TRUE)
        if (is.null(ylim)) ylim <- c(min(w.l), max(w.r))
    } else {
        if (is.null(xlim)) xlim <- c(min(w.l), max(w.r))
        if (is.null(ylim)) ylim <- range(rAdj, hdraw + offset, na.rm = TRUE)
    }
    if (beside && !vectorInput) w.m <- matrix(w.m, ncol = NC)
    if (!plot) return(w.m)
    if (!add) {
        plot.new()
        plot.window(xlim, ylim, log = log)
    }
    if (beside) {
        for (j in seq_len(NC)) for (i in seq_len(NR)) {
            if (horiz) rect(hdraw[i, j] + offset[i], w.l[(j - 1L) * NR + i],
                            rectbase + offset[i], w.r[(j - 1L) * NR + i], col = col[i], border = border)
            else rect(w.l[(j - 1L) * NR + i], rectbase + offset[i],
                      w.r[(j - 1L) * NR + i], hdraw[i, j] + offset[i], col = col[i], border = border)
        }
    } else {
        for (j in seq_len(NC)) for (i in seq_len(NR)) {
            if (horiz) rect(hdraw[i, j] + offset[j], w.l[j], hdraw[i + 1L, j] + offset[j], w.r[j], col = col[i], border = border)
            else rect(w.l[j], hdraw[i, j] + offset[j], w.r[j], hdraw[i + 1L, j] + offset[j], col = col[i], border = border)
        }
    }
    if (axisnames && !is.null(names.arg)) {
        at <- if (length(names.arg) == NC && !is.null(dim(w.m))) colMeans(w.m) else w.m
        axis(if (horiz) 2 else 1, at = as.vector(at), labels = names.arg)
    }
    if (axes) axis(if (horiz) 1 else 2)
    if (ann) title(main = main, sub = sub, xlab = xlab, ylab = ylab)
    invisible(w.m)
}
