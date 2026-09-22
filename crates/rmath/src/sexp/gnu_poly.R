poly <- function(x, ..., degree = 1, coefs = NULL, raw = FALSE, simple = FALSE)
{
    dots <- list(...)
    if(nd <- length(dots)) {
    dots_deg <- nd == 1L && length(dots[[1L]]) == 1L
    if(dots_deg) # unnamed degree, nothing else in '...'
            degree <- dots[[1L]]
        else return(polym(x, ..., degree = degree, coefs=coefs, raw = raw))
    }
    if(is.matrix(x)) {
    m <- unclass(as.data.frame(if(nd && dots_deg) x else cbind(x, ...)))
    return(do.call(polym, c(m, degree = degree, raw = raw,
                list(coefs=coefs))))
    }
    if(degree < 1)
        stop("'degree' must be at least 1")
    if(is.object(x) && mode(x) == "numeric" && !(is.factor(x) && !is.ordered(x)))
        x <- as.numeric(x) # for POSIXct, Date, ordered, but *not* (ordinary) factor
    if(raw) {
        Z <- outer(x, 1L:degree, `^`)
        colnames(Z) <- 1L:degree
    } else {
    if(is.null(coefs)) { # fitting
        if(anyNA(x)) stop("missing values are not allowed in 'poly'")
        if(degree >= length(unique(x)))
        stop("'degree' must be less than number of unique points")
        xbar <- mean(x)
        x <- x - xbar
        X <- outer(x, 0L:degree, `^`)
        QR <- qr(X)
        if(QR$rank < degree)
        stop("'degree' must be less than number of unique points")
        z <- QR$qr
        z <- z * (row(z) == col(z))
        Z <- qr.qy(QR, z)
        norm2 <- colSums(Z^2)
        alpha <- (colSums(x*Z^2)/norm2 + xbar)[1L:degree]
        norm2 <- c(1, norm2) # to use "common" code below
    } else {            # prediction
        alpha <- coefs$alpha; norm2 <- coefs$norm2
        Z <- matrix(1, length(x), degree + 1L) # Z[,1] == 1
        Z[, 2] <- x - alpha[1L]
        if(degree > 1)
        for(i in 2:degree)
            Z[, i+1] <- (x - alpha[i]) * Z[, i]  -
            (norm2[i+1] / norm2[i]) * Z[, i-1]
        }
        Z <- Z / rep(sqrt(norm2[-1L]), each = length(x))
        colnames(Z) <- 0L:degree
        Z <- Z[, -1, drop = FALSE]
        if(!simple) ## we may want to use the prediction to clone another prediction
            attr(Z, "coefs") <- list(alpha = alpha, norm2 = norm2)
    }
    if(simple) Z else structure(Z, degree = 1L:degree, class = c("poly", "matrix"))
}
