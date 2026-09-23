function(x, y, by = intersect(names(x), names(y)), by.x = by, by.y = by,
         all = FALSE, all.x = all, all.y = all,
         sort = TRUE, suffixes = c(".x", ".y"), no.dups = TRUE,
         incomparables = NULL, ...)
{
    x <- as.data.frame(x)
    y <- as.data.frame(y)
    if (length(by.x) == 0L || length(by.y) == 0L) {
        if (nrow(x) == 0L || nrow(y) == 0L)
            cbind(x[FALSE, ], y[FALSE, ])
        else {
            ij <- expand.grid(seq_len(nrow(x)), seq_len(nrow(y)))
            cbind(x[ij[, 1L], , drop = FALSE], y[ij[, 2L], , drop = FALSE])
        }
    } else {
        .Primitive("merge")(x, y, all = all, all.x = all.x, all.y = all.y)
    }
}
