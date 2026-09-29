function(x, digits = max(3L, getOption("digits") - 3L), zdigits = 4L, ...)
{
    if (is.null(digits)) digits <- max(3L, getOption("digits") - 3L)
    if (is.null(zdigits)) zdigits <- 4L
    if (is.character(x) || is.integer(x)) {
        print(unclass(x), ...)
        return(invisible(x))
    }
    if (inherits(x, "difftime")) {
        cat("Time differences in ", attr(x, "units"), "\n", sep = "")
        xx <- format(unclass(x), digits = digits)
    } else {
        xx <- format(x, digits = digits, zdigits = zdigits)
    }
    if (inherits(x, c("Date", "POSIXct"))) {
        print(xx, ...)
    } else {
        nm <- names(xx)
        if (is.null(nm)) nm <- names(x)
        width <- pmax(nchar(nm), nchar(xx), 7L)
        cat(paste0(format(nm, width = width, justify = "right"), " "), sep = "", "\n")
        cat(paste0(format(xx, width = width, justify = "right"), " "), sep = "", "\n")
    }
    invisible(x)
}
