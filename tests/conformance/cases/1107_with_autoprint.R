CO <- capture.output
stopifnot(identical(
    CO(withAutoprint({ x <- 1:2; cat("x=", x, "\n") }))[1],
    paste0(getOption("prompt"), "x <- 1:2")
))
stopifnot(grepl("1L, NA_integer_", CO(withAutoprint(x <- c(1L, NA_integer_, NA)))))
a <- CO(withAutoprint({ 1 + 1 }))
b <- CO(source(expr = list(quote(1 + 1)), echo = TRUE))
stopifnot(identical(a, b))
cat("ok\n")
