verify_tsp <- function() {
    x <- 1:4
    tsp(x) <- 1:3
    stopifnot(identical(tsp(x), c(1, 2, 3)))
    tsp(x) <- NULL
    stopifnot(is.null(tsp(x)), is.null(attributes(x)))
    error <- function(value, n = 4L) {
        x <- seq_len(n)
        tryCatch({ attr(x, "tsp") <- value; "accepted" },
                 error = function(e) conditionMessage(e))
    }
    stopifnot(identical(error(c(1, 2)),
                       "'tsp' attribute must be numeric of length three"))
    stopifnot(identical(error(c("1", "4", "1")),
                       "'tsp' attribute must be numeric of length three"))
    stopifnot(identical(error(c(1, 4, 0)),
                       "invalid time series parameters specified (0)"))
    stopifnot(identical(error(c(4, 1, 1)),
                       "invalid time series parameters specified (1)"))
    stopifnot(identical(error(c(1, 1, 1), 0L),
                       "cannot assign 'tsp' to zero-length vector"))
    stopifnot(identical(error(c(1, 1, 0), 0L),
                       "invalid time series parameters specified (0)"))
    stopifnot(identical(error(c(1, 4, NA_real_)), "accepted"))
    stopifnot(identical(error(c(NaN, 4, 1)), "accepted"))
    stopifnot(identical(error(c(1, 1, Inf)), "accepted"))
    stopifnot(identical(error(c(Inf, Inf, 1)), "accepted"))
    stopifnot(identical(error(c(1, 4, -Inf)),
                       "invalid time series parameters specified (0)"))
    m <- matrix(1:8, 4, 2)
    attr(m, "tsp") <- 1:3
    stopifnot(identical(attr(m, "tsp"), c(1, 2, 3)))
    values <- structure(1:3, class = "factor", levels = letters[1:3])
    stopifnot(identical(error(values),
                       "'tsp' attribute must be numeric of length three"))
    invisible(TRUE)
}

verify_drop <- function() {
    z <- array(42, c(1, 1), dimnames = list("r", "c"))
    before <- z
    y <- drop(z)
    stopifnot(identical(y, 42), is.null(attributes(y)), identical(z, before))
    single <- array(42, c(1, 1), dimnames = list("r", NULL))
    stopifnot(identical(names(drop(single)), "r"))
    three <- array(42, c(1, 1, 1), dimnames = list("r", NULL, "z"))
    stopifnot(is.null(names(drop(three))))
    one <- array(42, c(1), dimnames = list("r"))
    stopifnot(identical(names(drop(one)), "r"))
    m <- array(1:4, c(1, 4), dimnames = list("row", letters[1:4]))
    stopifnot(identical(drop(m), setNames(1:4, letters[1:4])))
    invisible(TRUE)
}
