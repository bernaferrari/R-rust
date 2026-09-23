function(x, y, by = intersect(names(x), names(y)), by.x = by, by.y = by,
         all = FALSE, all.x = all, all.y = all,
         sort = TRUE, suffixes = c(".x", ".y"), no.dups = TRUE,
         incomparables = NULL, ...)
{
    x <- as.data.frame(x)
    y <- as.data.frame(y)
    if (is.numeric(by.x) && any(by.x == 0L)) {
        x <- cbind(Row.names = row.names(x), x)
        by.x <- by.x + 1L
    }
    if (is.numeric(by.y) && any(by.y == 0L)) {
        y <- cbind(Row.names = row.names(y), y)
        by.y <- by.y + 1L
    }
    if (length(by.x) == 0L || length(by.y) == 0L) {
        if (nrow(x) == 0L || nrow(y) == 0L)
            cbind(x[FALSE, ], y[FALSE, ])
        else {
            ij <- expand.grid(seq_len(nrow(x)), seq_len(nrow(y)))
            cbind(x[ij[, 1L], , drop = FALSE], y[ij[, 2L], , drop = FALSE])
        }
    } else {
        if (is.numeric(by.x)) by.x <- names(x)[by.x]
        if (is.numeric(by.y)) by.y <- names(y)[by.y]
        .Primitive("merge")(x, y, by.x = by.x, by.y = by.y, all = all, all.x = all.x, all.y = all.y)
    }
}
