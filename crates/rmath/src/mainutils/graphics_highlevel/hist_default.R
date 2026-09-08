function(x, breaks = "Sturges", freq = NULL, probability = NULL,
         include.lowest = TRUE, right = TRUE, fuzz = 1e-7,
         density = NULL, angle = 45, col = "lightgray", border = NULL,
         main = paste("Histogram of", deparse(substitute(x))),
         xlim = NULL, ylim = NULL, xlab = deparse(substitute(x)),
         ylab = NULL, axes = TRUE, plot = TRUE, labels = FALSE,
         nclass = NULL, warn.unused = TRUE, panel.first = NULL, ...) {
    if (!is.numeric(x)) stop("'x' must be numeric")
    # Capture the caller's expression before filtering x; substitute(x) after
    # assignment would report the local symbol rather than (for example) foo.
    xname <- deparse(substitute(x))
    if (missing(main)) main <- paste("Histogram of", xname)
    if (missing(xlab)) xlab <- xname
    if (!is.null(panel.first)) stop("'panel.first' is not supported")
    if (!is.null(density)) stop("histogram density shading is not supported")
    if (length(angle) != 1L || angle != 45) stop("histogram shading angle is not supported")
    x <- x[is.finite(x)]
    n <- length(x)
    q7 <- function(z, p) {
        z <- sort(z)
        h <- (length(z) - 1) * p + 1
        j <- floor(h)
        if (j >= length(z)) z[length(z)] else z[j] + (h - j) * (z[j + 1L] - z[j])
    }
    med <- function(z) q7(z, .5)
    if (!is.null(nclass) && length(nclass) == 1L && missing(breaks)) breaks <- nclass

    use.br <- length(breaks) > 1L
    if (is.function(breaks)) {
        breaks <- breaks(x)
        use.br <- TRUE
    }
    if (use.br) {
        breaks <- sort(as.numeric(breaks))
    } else {
        if (!is.character(breaks)) {
            if (length(breaks) != 1L || !is.finite(breaks) || breaks < 1L)
                stop("invalid number of 'breaks'")
            nb <- min(as.integer(breaks), 1000000L)
        } else {
            method <- tolower(breaks[1L])
            if (n < 2L) nb <- 1L
            else if (method == "sturges") nb <- max(1L, ceiling(log2(max(1L, n)) + 1))
            else if (method == "scott") nb <- max(1L, ceiling((max(x) - min(x)) / (3.5 * sqrt(sum((x - mean(x))^2) / max(1, n - 1)) / n^(1/3))))
            else if (method == "fd" || method == "freedman-diaconis") nb <- max(1L, ceiling((max(x) - min(x)) / (2 * (q7(x, .75) - q7(x, .25)) / n^(1/3))))
            else stop("unknown 'breaks' algorithm")
        }
        if (n == 0L) stop("'x' must contain finite values")
        breaks <- pretty(range(x), n = nb, min.n = 1)
    }
    if (!is.numeric(breaks) || length(breaks) <= 1L)
        stop("invalid breakpoints produced by 'breaks'")
    breaks <- sort(breaks)
    nB <- length(breaks)
    h <- diff(breaks)
    if (any(h <= 0)) stop("'breaks' are not strictly increasing")
    equidist <- (max(h) - min(h)) < 1e-7 * mean(h)
    if (!is.null(probability) && !is.null(freq) && any(as.logical(probability) == as.logical(freq)))
        stop("'probability' is an alias for '!freq', however they differ.")
    freq1 <- if (is.null(freq)) {
        if (!is.null(probability)) !as.logical(probability)[1L] else equidist
    } else as.logical(freq)[1L]
    if (fuzz < 0) stop("fuzz must be non-negative")
    diddle <- fuzz * if (nB > 5L) med(h) else if (nB <= 3L) diff(range(x)) else min(h[h > 0])
    fuzzv <- if (right) c(if (include.lowest) -diddle else diddle, rep(diddle, nB - 1L))
             else c(rep(-diddle, nB - 1L), if (include.lowest) diddle else -diddle)
    fuzzybreaks <- breaks + fuzzv
    counts <- integer(nB - 1L)
    for (i in seq_along(counts)) {
        if (right) {
            left <- if (i == 1L && include.lowest) x >= fuzzybreaks[i] else x > fuzzybreaks[i]
            counts[i] <- sum(left & x <= fuzzybreaks[i + 1L])
        } else {
            upper <- if (i == nB - 1L && include.lowest) x <= fuzzybreaks[i + 1L] else x < fuzzybreaks[i + 1L]
            counts[i] <- sum(x >= fuzzybreaks[i] & upper)
        }
    }
    if (sum(counts) < n) stop("some 'x' not counted; maybe 'breaks' do not span range of 'x'")
    dens <- counts / (n * h)
    mids <- 0.5 * (breaks[-nB] + breaks[-1L])
    r <- structure(list(breaks = breaks, counts = counts, density = dens,
                        mids = mids, xname = xname, equidist = equidist),
                   class = "histogram")
    if (!plot) return(r)
    y <- if (freq1) counts else dens
    if (is.null(xlim)) xlim <- range(breaks)
    if (is.null(ylim)) ylim <- c(0, max(y) * 1.04)
    plot.new()
    plot.window(xlim, ylim)
    rect(breaks[-nB], rep(0, nB - 1L), breaks[-1L], y,
         col = col, border = border)
    if (axes) {
        axis(1)
        axis(2)
        box()
    }
    if (labels) text(mids, y, labels = if (is.logical(labels)) counts else labels, pos = 3)
    title(main = main, xlab = xlab, ylab = ylab)
    invisible(r)
}
