function(x, lag = 1L, differences = 1L, ...)
{
    if (length(lag) != 1L || length(differences) != 1L ||
        lag < 1L || differences < 1L)
        stop("'lag' and 'differences' must be integers >= 1")
    r <- unclass(x)
    i1 <- -seq_len(lag)
    if (is.matrix(x))
        for (i in seq_len(differences))
            r <- r[i1, , drop = FALSE] -
                r[seq_len(max(nrow(r) - lag, 0L)), , drop = FALSE]
    else
        for (i in seq_len(differences))
            r <- r[i1] - `length<-`(r, max(length(r) - lag, 0L))
    class(r) <- oldClass(x)
    r
}
