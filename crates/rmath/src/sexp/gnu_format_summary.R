function(x, digits = max(3L, getOption("digits") - 3L), zdigits = 4L, ...)
{
    if (is.null(digits)) digits <- max(3L, getOption("digits") - 3L)
    if (is.null(zdigits)) zdigits <- 4L
    if (is.character(x) || is.integer(x)) {
        format(unclass(x), ...)
    } else if (inherits(x, "POSIXct")) {
        c(format(unclass(x), digits = 0L),
          "NAs" = if (length(a <- attr(x, "NAs"))) as.character(a))
    } else if (inherits(x, c("Date", "difftime"))) {
        c(format(unclass(x), digits = digits),
          "NAs" = if (length(a <- attr(x, "NAs"))) as.character(a))
    } else {
        m <- match("NAs", names(x), 0L)
        nna <- x[m]
        if (m) x <- x[-m]
        finite <- is.finite(x)
        x[finite] <- zapsmall(x[finite], digits = digits + zdigits)
        xx <- format(unclass(x), digits = digits)
        if (m) c(xx, "NAs" = as.character(nna)) else xx
    }
}
