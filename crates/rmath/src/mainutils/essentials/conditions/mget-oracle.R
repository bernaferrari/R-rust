# Run with the executable pinned by oracle/r-oracle.json.
run <- function() {
    counter <<- 0L
    envir <- new.env()
    delayedAssign("x", { counter <<- counter + 1L; 42L },
                  eval.env = environment(), assign.env = envir)
    stopifnot(counter == 0L)
    values <- mget("x", envir, inherits = FALSE)
    stopifnot(identical(values, list(x = 42L)), counter == 1L)
    stopifnot(identical(mget(c("x", "x"), envir, inherits = FALSE),
                        structure(list(42L, 42L), names = c("x", "x"))), counter == 1L)
    counter <<- 0L
    envir$y <- 1L
    delayedAssign("x", { counter <<- counter + 1L; envir$y <- 9L; 42L },
                  eval.env = environment(), assign.env = envir)
    stopifnot(identical(mget(c("x", "y"), envir, inherits = FALSE), list(x = 42L, y = 9L)),
              counter == 1L)
    stopifnot(identical(mget("absent", envir, ifnotfound = list(17L)), list(absent = 17L)))
    cat("mget lazy/cache/sequential/fallback GNU controls: PASS\n")
}
run()
