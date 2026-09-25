{
axTicks <- function(side, axp = NULL, usr = NULL, log = NULL, nintLog = NULL)
{
    if(!(side <- as.integer(side)) %in% 1L:4L)
        stop("'side' must be in {1:4}")
    is.x <- side %% 2 == 1
    XY <- function(ch) paste0(if(is.x) "x" else "y", ch)
    if(is.null(axp)) axp <- par(XY("axp"))
    else if(!is.numeric(axp) || length(axp) != 3) stop("invalid 'axp'")
    if(is.null(log)) log <- par(XY("log"))
    else if(!is.logical(log) || anyNA(log)) stop("invalid 'log'")
    if(log && axp[3L] > 0) {
        if(!any((iC <- as.integer(axp[3L])) == 1L:3L))
            stop("invalid positive 'axp[3]'")
        if(is.null(usr)) usr <- par("usr")[if(is.x) 1:2 else 3:4]
        else if(!is.numeric(usr) || length(usr) != 2) stop("invalid 'usr'")
        if(is.null(nintLog)) nintLog <- par("lab")[2L - is.x]
        if(is.finite(nintLog)) {
            axisTicks(usr, log=log, axp=axp, nint=nintLog)
        } else {
	    if(needSort <- is.unsorted(usr)) {
		usr <- usr[2:1]; axp <- axp[2:1]
	    } else axp <- axp[1:2]
	    ii <- round(log10(axp))
	    x10 <- 10^((ii[1L] - (iC >= 2L)):ii[2L])
	    r <- switch(iC,
			x10,
			c(outer(c(1,  5), x10))[-1L],
			c(outer(c(1,2,5), x10))[-1L])
	    if(needSort)
		r <- rev(r)
            r[usr[1L] <= log10(r) & log10(r) <= usr[2L]]
        }
    } else {
	n <- as.integer(abs(axp[3L]) + 0.25)
	r <- seq.int(axp[1L], axp[2L], length.out = n + 1L)
	n. <- max(1L, n)
	N <- 100*n.
	r[abs(r) < abs(axp[2L]/N - axp[1L]/N)] <- 0
	r
    }
}
axTicks
}
