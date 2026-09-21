function(x, y = NULL, xlab = NULL, ylab = NULL, log = NULL, recycle = FALSE,
         setLab = TRUE)
{
    if (is.null(y)) {
        if (is.null(ylab)) ylab <- xlab
        if (is.language(x)) {
            if (inherits(x, "formula") && length(x) == 3) {
                y <- eval(x[[2L]], environment(x))
                x <- eval(x[[3L]], environment(x))
            } else stop("invalid first argument")
        } else if (is.complex(x)) {
            y <- Im(x)
            x <- Re(x)
        } else if (is.matrix(x) || is.data.frame(x)) {
            x <- data.matrix(x)
            if (ncol(x) == 1) {
                y <- x[, 1]
                x <- seq_along(y)
            } else {
                y <- x[, 2]
                x <- x[, 1]
            }
        } else if (is.list(x)) {
            if (all(c("x", "y") %in% names(x))) {
                y <- x[["y"]]
                x <- x[["x"]]
            } else stop("'x' is a list, but does not have components 'x' and 'y'")
        } else {
            if (is.factor(x)) x <- as.numeric(x)
            y <- x
            x <- seq_along(x)
        }
    }
    if (length(x) != length(y)) {
        if (recycle) {
            if ((nx <- length(x)) < (ny <- length(y))) x <- rep_len(x, ny)
            else y <- rep_len(y, nx)
        } else stop("'x' and 'y' lengths differ")
    }
    list(x = as.double(x), y = as.double(y), xlab = xlab, ylab = ylab)
}
