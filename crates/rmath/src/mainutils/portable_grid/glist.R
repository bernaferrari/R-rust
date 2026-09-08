function(...) {
    z <- list(...)
    for (x in z) if (!inherits(x, 'grob')) stop('only grobs allowed in gList')
    for (i in seq_along(z)) if (!is.null(z[[i]]$name)) names(z)[i] <- z[[i]]$name
    structure(z, class = 'gList')
}
