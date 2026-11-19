ordinary <- factor(rep(c("A", "B", "C"), 2), levels = c("A", "B", "C", "XX"))
is.na(ordinary) <- 3:4
explicit <- addNA(ordinary)
expected <- droplevels(explicit)
stopifnot(identical(explicit[, drop = TRUE], expected),
          identical(levels(explicit[, drop = TRUE]), c("A", "B", "C", NA_character_)),
          identical(ordinary[, drop = TRUE], droplevels(ordinary)),
          identical(explicit[, drop = FALSE], explicit))

mixed <- structure(c(1L, 4L, NA_integer_, 2L), levels = c("A", "B", "unused", NA_character_),
                   class = c("ordered", "factor"))
extract <- compiler::cmpfun(function(x, i) x[i, drop = TRUE])
expected <- droplevels(mixed[c(2L, 3L, 1L)])
actual <- extract(mixed, c(2L, 3L, 1L))
stopifnot(identical(actual, expected),
          identical(as.integer(actual), c(2L, 2L, 1L)),
          identical(levels(actual), c("A", NA_character_)),
          is.ordered(actual))
cat("Factor subsetting drops unused levels without changing labels\n")
