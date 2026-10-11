function(x, breaks, ..., xlab = deparse1(substitute(x)),
                        plot = TRUE, freq = FALSE,
                        start.on.monday = TRUE, format, right = TRUE)
{
    if(!inherits(x, "POSIXt")) stop("wrong method")
    force(xlab)
    x <- as.POSIXct(x)
    incr <- 1
    ## handle breaks ourselves
    if(missing(breaks))
	stop("Must specify 'breaks' in hist(<POSIXt>)")
    if (inherits(breaks, "POSIXt")) {
        breaks <- as.POSIXct(breaks)
        d <- min(abs(diff(unclass(breaks))))
        if(d > 60) incr <- 60
        if(d > 3600) incr <- 3600
        if(d > 86400) incr <- 86400
        if(d > 86400*7) incr <- 86400*7
        if(d > 86400*28) incr <- 86400*28
        if(d > 86400*366) incr <- 86400*366
        num.br <- FALSE
    } else {
        num.br <- is.numeric(breaks) && length(breaks) == 1L
        if(num.br) {
            ## specified number of breaks
        } else if(is.character(breaks) && length(breaks) == 1L) {
            valid <-
                pmatch(breaks,
                       c("secs", "mins", "hours", "days", "weeks",
                         "months", "years", "quarters"))
            if(is.na(valid)) stop("invalid specification of 'breaks'")
            start <- as.POSIXlt(min(x, na.rm = TRUE))
            ## may alter later
            ## we need to invalidate isdst whenever we play with components
            incr <- 1
            if(valid > 1L) { start$sec <- 0; incr <- 59.99 }
            if(valid > 2L) { start$min <- 0L; incr <- 3600 - 1 }
            if(valid > 3L) { start$hour <- 0L; incr <- 86400 - 1 }
            if(valid > 4L) { start$isdst <- -1L}
            if(valid == 5L) { # "weeks"
                start$mday <- start$mday - start$wday
                if(start.on.monday)
                    start$mday <- start$mday + if(start$wday > 0L) 1L else -6L
                incr <- 7*86400
            }
            if(valid == 6L) { # "months"
                start$mday <- 1L
                end <- as.POSIXlt(max(x, na.rm = TRUE))
                end <- as.POSIXlt(end + (31 * 86400))
                end$mday <- 1L
                end$isdst <- -1L
                breaks <- seq(start, end, "months")
                ind <- seq_along(breaks[-1L])
                if (right)
                    breaks[ind] <- breaks[ind] - 86400
		if (missing(format)) format <- "%Y-%m-%d"
            } else if(valid == 7L) { # "years"
                start$mon <- 0L
                start$mday <- 1L
                end <- as.POSIXlt(max(x, na.rm = TRUE))
                end <- as.POSIXlt(end + (366 * 86400))
                end$mon <- 0L
                end$mday <- 1L
                end$isdst <- -1L
                breaks <- seq(start, end, "years")
                ind <- seq_along(breaks[-1L])
                if (right)
                    breaks[ind] <- breaks[ind] - 86400
		if (missing(format)) format <- "%Y-%m-%d"
            } else if(valid == 8L) { # "quarters"
                qtr <- rep(c(0L, 3L, 6L, 9L), each = 3L)
                start$mon <- qtr[start$mon + 1L]
                start$mday <- 1L
                end <- as.POSIXlt(max(x, na.rm = TRUE))
                end <- as.POSIXlt(end + (93 * 86400))
                end$mon <- qtr[end$mon + 1L]
                end$mday <- 1L
                end$isdst <- -1L
                breaks <- seq(start, end, "3 months")
                ind <- seq_along(breaks[-1L])
                if (right)
                    breaks[ind] <- breaks[ind] - 86400
		if (missing(format)) format <- "%Y-%m-%d"
           } else { # anything from "secs" to "weeks"
                maxx <- max(x, na.rm = TRUE)
                breaks <- seq(start, maxx + incr, breaks)
                if (length(breaks) > 2L)
                breaks <- breaks[seq_len(1L + max(which(breaks < maxx)))]
            }
        }
        else stop("invalid specification of 'breaks'")
    }
    res <- hist.default(unclass(x), unclass(breaks), plot = FALSE,
                        warn.unused = FALSE, right = right, ...)
    res$equidist <- TRUE # years are of uneven lengths
    res$xname <- xlab
    if(plot) {
        ## swallow '...' args only for hist.default() above & separate out 'axes':
        myplot <- function(res, xlab, freq, format, breaks,
                           include.lowest, fuzz, # <<- swallowed here
                           density = NULL, angle = 45, col = "lightgray",
                           border = NULL, lty = NULL,
                           labels = FALSE,
                           axes = TRUE, xaxt = par("xaxt"), ...)
        {
            plot(res, xlab = xlab, axes = FALSE, freq = freq,
                 density = density, angle = angle, col = col,
                 border = border, lty = lty,
		 labels = labels, ...)
	    if(axes) {
		axis(2, ...)
		if(xaxt != "n") {
		    if(num.br)
                        breaks <- as.POSIXct(res$breaks,
                                             origin = "1970-01-01")
		    axis.POSIXct(1, at = breaks,  format = format, ...)
					# '...' : e.g. cex.axis
		}
	    }
        }
        myplot(res, xlab, freq, format, breaks, ...)
     }
    invisible(res)
}
