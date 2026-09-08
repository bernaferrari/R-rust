function(x, ..., range = 1.5, width = NULL, varwidth = FALSE,
         notch = FALSE, warnN = TRUE, outline = TRUE, names = NULL,
         plot = TRUE, border = "black", col = "lightgray", log = "",
         ann = TRUE, horizontal = FALSE, add = FALSE, at = NULL,
         main = NULL, sub = NULL, xlab = NULL, ylab = NULL,
         xlim = NULL, ylim = NULL, axes = TRUE) {
    if (notch) stop("notches are not supported")
    if (!is.null(width) || varwidth) stop("boxplot width and varwidth are not supported")
    if (!is.character(log) || length(log) > 1L || (length(log) == 1L && nzchar(log))) stop("logarithmic boxplots are not supported")
    if (length(range) != 1L || !is.numeric(range) || is.na(range) || range < 0)
        stop("'range' must be a non-negative scalar")
    # As in the upstream default method, additional vectors supplied through
    # `...` are additional groups.  Keep them here rather than silently
    # dropping them when the first argument is a vector.
    groups <- if (is.list(x)) c(x, list(...)) else c(list(x), list(...))
    if (length(groups) == 0L) stop("invalid first argument")
    if (is.null(names)) {
        names <- names(groups)
        if (is.null(names)) names <- as.character(seq_along(groups))
    }
    one <- function(z) {
        z <- sort(z[!is.na(z)])
        n <- length(z)
        if (n == 0L) return(list(stats = rep(NA_real_, 5L), n = 0L, conf = c(NA_real_, NA_real_), out = numeric()))
        med <- if (n %% 2L == 1L) z[(n + 1L) / 2L] else (z[n / 2L] + z[n / 2L + 1L]) / 2
        # Tukey's hinges (the quartiles returned by fivenum), with a
        # fractional index averaged between adjacent observations.
        h <- floor((n + 3) / 2) / 2
        j <- floor(h)
        q1 <- if (h == j) z[j] else (z[j] + z[j + 1L]) / 2
        h <- n + 1 - h
        j <- floor(h)
        q3 <- if (h == j) z[j] else (z[j] + z[j + 1L]) / 2
        iqr <- q3 - q1
        lo <- q1 - range * iqr; hi <- q3 + range * iqr
        # coef == 0 is explicitly supported by boxplot.stats: whiskers span
        # the full sample and no observations are classified as outliers.
        inside <- if (range == 0) rep(TRUE, n) else z >= lo & z <= hi
        list(stats = c(min(z[inside]), q1, med, q3, max(z[inside])),
             n = n, conf = c(med - 1.58 * iqr / sqrt(n), med + 1.58 * iqr / sqrt(n)),
             out = z[!inside])
    }
    n <- length(groups)
    stats <- matrix(NA_real_, 5L, n)
    conf <- matrix(NA_real_, 2L, n)
    ng <- numeric(n)
    out <- numeric(); group <- numeric()
    for (i in seq_len(n)) {
        z <- one(groups[[i]])
        stats[, i] <- z$stats; conf[, i] <- z$conf; ng[i] <- z$n
        if (length(z$out)) { out <- c(out, z$out); group <- c(group, rep(i, length(z$out))) }
    }
    result <- list(stats = stats, n = ng, conf = conf, out = out,
                   group = group, names = names)
    if (!plot) return(result)
    if (is.null(at)) at <- seq_len(n)
    if (length(at) != n || any(!is.finite(at))) stop("invalid 'at'")
    yr <- range(c(stats, out), finite = TRUE)
    if (horizontal) {
        if (is.null(xlim)) xlim <- yr
        if (is.null(ylim)) ylim <- range(at) + c(-.5, .5)
        if (!add) { plot.new(); plot.window(xlim, ylim) }
    } else {
        if (is.null(xlim)) xlim <- range(at) + c(-.5, .5)
        if (is.null(ylim)) ylim <- yr
        if (!add) { plot.new(); plot.window(xlim, ylim) }
    }
    for (i in seq_len(n)) {
        position <- at[i]
        if (horizontal) {
            rect(stats[2L, i], position - .35, stats[4L, i], position + .35, col = col, border = border)
            lines(c(stats[1L, i], stats[2L, i]), c(position, position)); lines(c(stats[4L, i], stats[5L, i]), c(position, position)); lines(c(stats[3L, i], stats[3L, i]), c(position - .35, position + .35))
            if (outline && length(out)) points(out[group == i], rep(position, sum(group == i)))
        } else {
            rect(position - .35, stats[2L, i], position + .35, stats[4L, i], col = col, border = border)
            lines(c(position, position), c(stats[1L, i], stats[2L, i])); lines(c(position, position), c(stats[4L, i], stats[5L, i])); lines(c(position - .35, position + .35), c(stats[3L, i], stats[3L, i]))
            if (outline && length(out)) points(rep(position, sum(group == i)), out[group == i])
        }
    }
    if (axes) {
        axis(if (horizontal) 2 else 1, at = at, labels = names)
        axis(if (horizontal) 1 else 2)
        box()
    }
    if (ann) title(main = main, sub = sub,
                   xlab = if (is.null(xlab)) if (horizontal) "" else NULL else xlab,
                   ylab = if (is.null(ylab)) if (horizontal) NULL else "" else ylab)
    invisible(result)
}
