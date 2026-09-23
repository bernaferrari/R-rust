function (x, lag = 1, differences = 1, ...)
{
    if (lag < 1 || differences < 1)
        stop("bad value for 'lag' or 'differences'")
    if (lag * differences >= NROW(x))
        return(if (is.matrix(x)) x[0L, , drop = FALSE] else x[0L])
    r <- if (is.matrix(x)) unclass(x) else as.vector(x)
    for (i in seq_len(differences)) {
        n <- NROW(r)
        r <- if (is.matrix(r))
            r[(lag + 1L):n, , drop = FALSE] - r[seq_len(n - lag), , drop = FALSE]
        else
            r[(lag + 1L):n] - r[seq_len(n - lag)]
    }
    xtsp <- tsp(x)
    ts(r, end = xtsp[2L], frequency = xtsp[3L])
}
