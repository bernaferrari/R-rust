{
format.AsIs <- function(x, width = 12, ...)
{
    if(is.character(x) || (is.atomic(x) && is.matrix(x)))
	return(format.default(x, ...))
    if(is.null(width)) width <- 12L
    rvec <- vapply(x, function(y) {
        ## need to remove class AsIs to avoid an infinite loop.
        cl <- oldClass(y)
        if(m <- match("AsIs", cl, 0L)) oldClass(y) <- cl[-m]
        toString(y, width = width, ...)
    }, "")
    ## AsIs might be around a matrix, which is not a class.
    dim(rvec) <- dim(x)
    dimnames(rvec) <- dimnames(x)
    format.default(rvec, justify = "right")
}
format.AsIs
}
