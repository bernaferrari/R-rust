# Primitive anyNA exact/partial argument matching, validation order, and
# internal-generic dispatch ordering.
expect_error <- function(expr, expected) {
  actual <- tryCatch({ force(expr); NA_character_ }, error = function(e) conditionMessage(e))
  stopifnot(identical(actual, expected))
}

stopifnot(anyNA(c(1, NA), recursive = FALSE))
stopifnot(anyNA(c(1, NA), rec = FALSE))
stopifnot(anyNA(c(1, NA), r = FALSE))
stopifnot(anyNA(x = c(1, NA), recursive = FALSE))
stopifnot(anyNA(rec = FALSE, x = c(1, NA)))
expect_error(anyNA(c(NA), recursive = FALSE, recursive = TRUE), "anyNA takes 1 or 2 arguments")
expect_error(anyNA(c(NA), foo = FALSE), "unused argument (foo = FALSE)")
expect_error(anyNA(c(NA), recursive = ), "argument 2 is empty")
expect_error(anyNA(), "anyNA takes 1 or 2 arguments")
expect_error(anyNA(recursive = FALSE), "supplied argument name 'recursive' does not match 'x'")
expect_error(anyNA(rec = FALSE), "supplied argument name 'rec' does not match 'x'")
expect_error(anyNA(foo = FALSE), "supplied argument name 'foo' does not match 'x'")
expect_error(anyNA(x = ), "argument 1 is empty")
expect_error(anyNA(, recursive = FALSE), "argument 1 is empty")

method_called <- FALSE
anyNA.probe <- function(x, recursive = FALSE, ...) {
  method_called <<- TRUE
  identical(names(list(...)), "foo")
}
probe <- structure(NA_real_, class = "probe")
stopifnot(anyNA(probe, foo = TRUE))
method_called <- FALSE
expect_error(anyNA(probe, recursive = FALSE, foo = TRUE), "anyNA takes 1 or 2 arguments")
stopifnot(!method_called)
cat("anyNA argument matching ok\n")
