function(expr, from = NULL, to = NULL, n = 101, add = FALSE,
         type = "l", xname = "x", xlab = xname,
         ylab = NULL, log = NULL, xlim = NULL, ...)
{
    sexpr <- substitute(expr)
    if (is.name(sexpr)) {
        expr <- call(as.character(sexpr), as.name(xname))
    } else {
        if (!(is.call(sexpr) || is.expression(sexpr)) ||
            !(xname %in% all.vars(sexpr)))
            stop(gettextf("'expr' must be a function, or a call or an expression containing '%s'", xname), domain = NA)
        expr <- sexpr
    }
    addF <- isFALSE(add)
    if (is.null(ylab)) ylab <- deparse1(expr)
    if (is.null(from) || is.null(to)) {
        xl <- if (!is.null(xlim)) xlim else c(0, 1)
        if (is.null(from)) from <- xl[1L]
        if (is.null(to)) to <- xl[2L]
    }
    lg <- if (length(log)) log else ""
    if (grepl("x", lg, fixed = TRUE)) {
        if (from <= 0 || to <= 0)
            stop("'from' and 'to' must be > 0 with log=\"x\"")
        x <- exp(seq.int(log(from), log(to), length.out = n))
    } else x <- seq.int(from, to, length.out = n)
    ll <- list(x = x)
    names(ll) <- xname
    y <- eval(expr, envir = ll, enclos = parent.frame())
    if (length(y) != length(x))
        stop("'expr' did not evaluate to an object of length 'n'")
    invisible(list(x = x, y = y))
}
