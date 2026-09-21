function(from, to, by, length.out = NULL, along.with = NULL, ...)
{
    if (!missing(along.with)) {
        length.out <- length(along.with)
    }  else if (!is.null(length.out)) {
        if (length(length.out) != 1L) stop(gettextf("'%s' must be of length 1", "length.out"), domain=NA)
        length.out <- ceiling(length.out)
    }
    missing_arg <- names(which(c(from = missing(from), to = missing(to),
                                 length.out = is.null(length.out), by = missing(by))))
    if(length(missing_arg) != 1L)
        stop("exactly three of 'to', 'from', 'by' and 'length.out' / 'along.with' must be specified")
    if (missing_arg != "to") {
        if (!inherits(to, "POSIXt")) stop(gettextf("'%s' must be a \"%s\" object", "to", "POSIXt"), domain=NA)
        if (length(to) != 1L) stop(gettextf("'%s' must be of length 1", "to"), domain=NA)
        cto <- as.POSIXct(to)
        tz <- attr(cto, "tzone")
    }
    if (missing_arg != "from") {
        if (!inherits(from, "POSIXt")) stop(gettextf("'%s' must be a \"%s\" object", "from", "POSIXt"), domain=NA)
        if (length(from) != 1L) stop(gettextf("'%s' must be of length 1", "from"), domain=NA)
        cfrom <- as.POSIXct(from)
        tz <- attr(cfrom, "tzone")
    }
    if (missing_arg == "by") {
        from <- unclass(as.POSIXct(from))
        to   <- unclass(as.POSIXct(to))
        res <- seq.int(from, to, length.out = length.out)
        return(.POSIXct(res, tz = attr(from, "tzone")))
    }
    if (length(by) != 1L) stop(gettextf("'%s' must be of length 1", "by"), domain=NA)
    valid <- 0L
    if (inherits(by, "difftime")) {
        units(by) <- "secs"
        by <- as.vector(by)
    } else if(is.character(by)) {
        by2 <- strsplit(by, " ", fixed = TRUE)[[1L]]
        if(length(by2) > 2L || length(by2) < 1L)
            stop("invalid 'by' string")
        valid <- pmatch(by2[length(by2)],
                        c("secs", "mins", "hours", "days", "weeks",
                          "months", "years", "DSTdays", "quarters"))
        if(is.na(valid)) stop("invalid string for 'by'")
        if(valid <= 5L) {
            by <- c(1, 60, 3600, 86400, 7*86400)[valid]
            if (length(by2) == 2L) by <- by * as.integer(by2[1L])
        } else
            by <- if(length(by2) == 2L) as.integer(by2[1L]) else 1L
    } else if(!is.numeric(by)) stop("invalid mode for 'by'")
    if(is.na(by)) stop("'by' is NA")

    if(valid <= 5L) {
       res <- switch(missing_arg,
           from       = seq.int(to   = unclass(cto),   by = by,           length.out = length.out),
           to         = seq.int(from = unclass(cfrom), by = by,           length.out = length.out),
           length.out = seq.int(from = unclass(cfrom), to = unclass(cto), by = by)
       )
       return(.POSIXct(res, tz))
    }
    lres <- as.POSIXlt(if (missing_arg != "from") from else to)
    if (missing_arg == "length.out") lto <- as.POSIXlt(to)
    if(valid == 7L) {
        lres$year <- switch(missing_arg,
          from       = seq.int(to   = lres$year, by = by, length.out = length.out),
          to         = seq.int(from = lres$year, by = by, length.out = length.out),
          length.out = seq.int(from = lres$year, to = lto$year, by = by)
        )
    } else if(valid %in% c(6L, 9L)) {
        if (valid == 9L) by <- by * 3
        lres$mon <- switch(missing_arg,
          from       = seq.int(to   = lres$mon, by = by, length.out = length.out),
          to         = seq.int(from = lres$mon, by = by, length.out = length.out),
          length.out = seq.int(lres$mon, 12*(lto$year - lres$year) + lto$mon, by)
        )
    } else if(valid == 8L) {
        lres$mday <- switch(missing_arg,
          from       = seq.int(to   = lres$mday, by = by, length.out = length.out),
          to         = seq.int(from = lres$mday, by = by, length.out = length.out),
          length.out = seq.int(lres$mday, by = by,
                               length.out = 2L + floor((unclass(cto) - unclass(cfrom))/(by * 86400)))
        )
    }
    lres$isdst <- -1L
    res <- as.POSIXct(lres)
    if(missing_arg == "length.out")
        res[if(by > 0) res <= cto else res >= cto]
    else
        res
}
