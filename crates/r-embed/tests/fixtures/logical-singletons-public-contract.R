local({
    a <- is.numeric(1)
    names(a) <- "kept"
    stopifnot(identical(names(a), "kept"), is.null(names(is.numeric(1))))
    b <- is.numeric("text")
    attr(b, "probe") <- 17L
    stopifnot(identical(attr(b, "probe"), 17L), is.null(attr(is.numeric("text"), "probe")))
    c <- identical(1L, 1L)
    c[1L] <- FALSE
    stopifnot(identical(c, FALSE), identical(identical(1L, 1L), TRUE))
    missing <- as.logical(NA_integer_)
    missing[1L] <- TRUE
    stopifnot(identical(missing, TRUE), identical(as.logical(NA_integer_), NA))
    TRUE
})
