function(x, arr.ind = FALSE, useNames = TRUE)
{
    wh <- .Internal(which(x))
    if (isTRUE(arr.ind) && !is.null(d <- dim(x)))
	arrayInd(wh, d, dimnames(x), useNames=useNames) else wh
}
