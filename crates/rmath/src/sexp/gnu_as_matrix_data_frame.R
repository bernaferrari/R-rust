function (x, rownames.force = NA, ...)
{
    dm <- dim(x)
    rn <- if(rownames.force %in% FALSE) NULL
	  else if(rownames.force %in% TRUE || .row_names_info(x) > 0L)
              row.names(x) # else NULL
    dn <- list(rn, names(x))
    if(any(dm == 0L))
	return(array(NA, dim = dm, dimnames = dn))
    p <- dm[2L] # >= 1
    pseq <- seq_len(p)
    n <- dm[1L]
    X <- unclass(x) # will contain the result;
    ## the "big question" is if we return a numeric or a character matrix
    non.numeric <- non.atomic <- FALSE
    all.logical <- TRUE
    for (j in pseq) {
	xj <- X[[j]]
	if(inherits(xj, "data.frame"))# && ncol(xj) > 1L)
	    X[[j]] <- xj <- as.matrix(xj)
        j.logic <- is.logical(xj)
        if(all.logical && !j.logic) all.logical <- FALSE
	if(length(levels(xj)) > 0L || !(j.logic || is.numeric(xj) || is.complex(xj))
	   || (!is.null(cl <- attr(xj, "class")) && # numeric classed objects to format:
	       any(cl %in% c("Date", "POSIXct", "POSIXlt"))))
	    non.numeric <- TRUE
	if(!is.atomic(xj) && !inherits(xj, "POSIXlt"))
	    non.atomic <- TRUE
    }
    if(non.atomic) {
	for (j in pseq) {
	    xj <- X[[j]]
	    if(!is.recursive(xj))
		X[[j]] <- as.list(as.vector(xj))
	}
    } else if(all.logical) {
        ## do nothing for logical columns if a logical matrix will result.
    } else if(non.numeric) {
	for (j in pseq) {
	    if (is.character(X[[j]]))
		next
	    else if(is.logical(xj <- X[[j]]))
		xj <- as.character(xj) # not format(), takes care of NAs too
	    else {
		miss <- is.na(xj)
		xj <- if(length(levels(xj))) as.vector(xj) else format(xj)
		is.na(xj) <- miss
	    }
            X[[j]] <- xj
	}
    }
    ## These coercions could have changed the number of columns
    ## (e.g. class "Surv" coerced to character),
    ## so only now can we compute collabs.
    collabs <- as.list(dn[[2L]])
    for (j in pseq) {
        xj <- X[[j]]
        dj <- dim(xj)
        if(length(dj) == 2L && dj[2L] > 0L) { # matrix with > 0 col
            if(!length(dnj <- colnames(xj))) dnj <- seq_len(dj[2L])
            collabs[[j]] <-
                if(length(collabs)) {
                    if(dj[2L] > 1L)
                        paste(collabs[[j]], dnj, sep = ".")
                    else if(is.character(collabs[[j]])) collabs[[j]]
                    else dnj
                }
                else dnj
        }
    }
    nc <- vapply(X, NCOL, numeric(1), USE.NAMES=FALSE)
    X <- unlist(X, recursive = FALSE, use.names = FALSE)
    dim(X) <- c(n, length(X)/n)
    dimnames(X) <- list(dn[[1L]], unlist(collabs[nc > 0], use.names = FALSE))
    X
}
