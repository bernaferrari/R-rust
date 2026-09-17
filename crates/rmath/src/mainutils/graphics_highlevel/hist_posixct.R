function(x, breaks, ..., xlab = deparse(substitute(x)),
         plot = TRUE, freq = FALSE,
         start.on.monday = TRUE, format, right = TRUE)
{
    if (!inherits(x, "POSIXt")) stop("wrong method")
    if (missing(breaks))
        stop("Must specify 'breaks' in hist(<POSIXt>)")
    x <- as.POSIXct(x)
    incr <- 1
    if (inherits(breaks, "POSIXt")) {
        breaks <- as.POSIXct(breaks)
    } else if (is.numeric(breaks) && length(breaks) == 1L) {
        ## number of breaks
    } else if (is.character(breaks) && length(breaks) == 1L) {
        valid <- pmatch(breaks,
                        c("secs", "mins", "hours", "days", "weeks",
                          "months", "years", "quarters"))
        if (is.na(valid)) stop("invalid specification of 'breaks'")
        start <- as.POSIXlt(min(x, na.rm = TRUE))
        incr <- 1
        if (valid > 1L) { start$sec <- 0; incr <- 59.99 }
        if (valid > 2L) { start$min <- 0L; incr <- 3600 - 1 }
        if (valid > 3L) { start$hour <- 0L; incr <- 86400 - 1 }
        if (valid > 4L) { start$isdst <- -1L }
        if (valid == 5L) {
            start$mday <- start$mday - start$wday
            if (start.on.monday)
                start$mday <- start$mday + if (start$wday > 0L) 1L else -6L
            incr <- 7 * 86400
        }
        if (valid == 6L) {
            start$mday <- 1L
            end <- as.POSIXlt(max(x, na.rm = TRUE))
            end <- as.POSIXlt(end + (31 * 86400))
            end$mday <- 1L
            end$isdst <- -1L
            breaks <- seq(start, end, "months")
            ind <- seq_along(breaks[-1L])
            if (right) breaks[ind] <- breaks[ind] - 86400
        } else if (valid == 7L) {
            start$mon <- 0L
            start$mday <- 1L
            end <- as.POSIXlt(max(x, na.rm = TRUE))
            end <- as.POSIXlt(end + (366 * 86400))
            end$mon <- 0L
            end$mday <- 1L
            end$isdst <- -1L
            breaks <- seq(start, end, "years")
            ind <- seq_along(breaks[-1L])
            if (right) breaks[ind] <- breaks[ind] - 86400
        } else if (valid == 8L) {
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
            if (right) breaks[ind] <- breaks[ind] - 86400
        } else {
            maxx <- max(x, na.rm = TRUE)
            breaks <- seq(start, maxx + incr, breaks)
            if (length(breaks) > 2L)
                breaks <- breaks[seq_len(1L + max(which(breaks < maxx)))]
        }
    } else stop("invalid specification of 'breaks'")
    hist.default(unclass(x), unclass(breaks), plot = FALSE,
                 warn.unused = FALSE, right = right, ...)
}
