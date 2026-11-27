# R-level utils package definitions, evaluated into the utils namespace at
# bootstrap exactly as GNU populates package code. Semantics mirror GNU R's
# utils/R sources; implementations are kept dependency-minimal for the
# primitive surface the pure world carries.

modifyList <- function (x, val, keep.null = FALSE)
{
    stopifnot(is.list(x), is.list(val))
    xnames <- names(x)
    vnames <- names(val)
    vnames <- vnames[nzchar(vnames)]
    if (keep.null) {
        for (v in vnames) x[v] <- list(val[v])
    }
    else {
        for (v in vnames) x[[v]] <- if (v %in% xnames && is.list(x[[v]]) &&
            is.list(val[[v]]))
            modifyList(x[[v]], val[[v]])
        else val[[v]]
    }
    x
}
