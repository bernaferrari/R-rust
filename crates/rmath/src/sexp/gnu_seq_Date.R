function(from, to, by, length.out = NULL, along.with = NULL, ...)
{
    if (!missing(along.with)) {
        length.out <- length(along.with)
    } else if(!is.null(length.out)) {
        if (length(length.out) != 1L) stop(gettextf("'%s' must be of length 1", "length.out"), domain=NA)
        length.out <- ceiling(length.out)
    }
    if(missing(by)) {
        if(((mTo <- missing(to)) & (mFr <- missing(from))))
            stop("without 'by', at least one of 'to' and 'from' must be specified")
        if((mTo || mFr) && is.null(length.out))
            stop("without 'by', when one of 'to', 'from' is missing, 'length.out' / 'along.with' must be specified")
        if(!mFr) from <- as.integer(as.Date(from))
        if(!mTo) to   <- as.integer(as.Date(to))
        res <- if(mFr) seq.int(to = to,  length.out = length.out)
          else if(mTo) seq.int(from,     length.out = length.out)
          else         seq.int(from, to, length.out = length.out)
        return(.Date(res))
    }
    if (length(by) != 1L) stop(gettextf("'%s' must be of length 1", "by"), domain=NA)
    missing_arg <- names(which(c(from = missing(from), to = missing(to),
                                 length.out = is.null(length.out))))
    if(length(missing_arg) != 1L)
        stop("given 'by', exactly two of 'to', 'from' and 'length.out' / 'along.with' must be specified")
    if (inherits(by, "difftime")) {
        units(by) <- "days"
        by <- as.vector(by)
    } else if(is.character(by)) {
        nby2 <- length(by2 <- strsplit(by, " ", fixed = TRUE)[[1L]])
        if(nby2 > 2L || nby2 < 1L)
            stop("invalid 'by' string")
        bys <- c("days", "weeks", "months", "quarters", "years")
        valid <- pmatch(by2[nby2], bys)
        if(is.na(valid)) stop("invalid string for 'by'")
        by <- bys[valid]
        if(valid > 2L) {
            if (nby2 == 2L) by <- paste(by2[1L], by)
            res <- switch(missing_arg,
              from       = seq(to   = as.POSIXlt(to),   by = by,             length.out = length.out),
              to         = seq(from = as.POSIXlt(from), by = by,             length.out = length.out),
              length.out = seq(from = as.POSIXlt(from), to = as.POSIXlt(to), by = by)
            )
            return(as.Date(res))
        }
        by <- c(1L, 7L)[valid]
        if (nby2 == 2L) by <- by * as.integer(by2[1L])
    }
    else if(!is.numeric(by)) stop("invalid mode for 'by'")
    if(is.na(by)) stop("'by' is NA")

    res <- switch(missing_arg,
        from       = seq.int(to   = unclass(to),   by = by,          length.out = length.out),
        to         = seq.int(from = unclass(from), by = by,          length.out = length.out),
        length.out = seq.int(from = unclass(from), to = unclass(to), by = by)
    )
    .Date(res)
}
