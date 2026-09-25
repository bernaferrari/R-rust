function(formula, data = parent.frame(), ..., subset,
         ylab = varnames[response], ask = dev.interactive())
{
    m <- match.call(expand.dots = FALSE)
    eframe <- parent.frame()
    md <- eval(m$data, eframe)
    if (is.matrix(md)) m$data <- md <- as.data.frame(data)
    dots <- lapply(m$..., eval, md, eframe)
    nmdots <- names(dots)
    for(nm in nmdots[match(c("main", "sub", "xlab"), nmdots, 0L)])
        dots[[nm]] <- enquote(dots[[nm]])
    if(!missing(ylab)) ylab <- enquote(ylab)

    m$ylab <- m$... <- m$ask <- NULL
    subset.expr <- m$subset
    m$subset <- NULL
    m <- as.list(m)
    m[[1L]] <- stats::model.frame.default
    m <- as.call(c(m, list(na.action = NULL)))
    mf <- eval(m, eframe)
    if (!missing(subset)) {
	s <- eval(subset.expr, data, eframe)
	l <- nrow(mf)
	dosub <- function(x) if (length(x) == l) x[s] else x
	dots <- lapply(dots, dosub)
	mf <- mf[s, , drop=FALSE]
    }
    horizontal <- FALSE
    if ("horizontal" %in% names(dots)) horizontal <- dots[["horizontal"]]
    response <- attr(attr(mf, "terms"), "response")
    if (is.null(response) || length(response) == 0L) response <- 0L
    if (response) {
	varnames <- names(mf)
	y <- mf[[response]]
	funname <- "plot"
	xn <- varnames[-response]
        if(length(xn)) {
            for (i in xn) {
                do.call(funname, c(list(mf[[i]], y), dots))
	    }
	} else do.call(funname, c(list(y), dots))
    } else do.call("plot.data.frame", c(list(mf), dots))
    invisible()
}
