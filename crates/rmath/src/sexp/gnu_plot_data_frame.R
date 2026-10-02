function (x, formula = NULL, ...)
{
    plot2 <- function(x, xlab=names(x)[1L], ylab=names(x)[2L], ...)
        plot(x[[1L]], x[[2L]], xlab=xlab, ylab=ylab, ...)

    if(!is.data.frame(x))
	stop("'plot.data.frame' applied to non data frame")
    if (!is.null(formula)) {
        ccall <- match.call()
        ccall[2:3] <- ccall[3:2]
        names(ccall)[2:3] <- c("", "data") # formula in plot.formula cannot be named
        ccall[[2]] <- stats::as.formula(formula)
        ccall[[1]] <- quote(graphics::plot)
        eval.parent(ccall)
    }
    else if(ncol(x) == 1) {
        x1 <- x[[1L]]
        if(length(class(x1)) && class(x1)[1L] %in% c("integer", "numeric")) {# is.numeric(.) TRUE for 'ts'
            ## the special case: *not* using plot() method
            stripchart(x1, ...)
        } else plot(x1, ...) # factor, ts, complex ...
    } else if(ncol(x) == 2) {
        plot2(x, ...)
    } else {
	pairs(data.matrix(x), ...)
    }
}
