# Independent controls for the pinned GNU oracle in oracle/r-oracle.json.
expected <- list(logical(), integer(), double(), complex(), character(), raw(),
                 list(), expression(NULL), NULL, NULL)
modes <- c("logical", "integer", "double", "complex", "character", "raw",
           "list", "expression", "pairlist", "any")
for (i in seq_along(modes)) stopifnot(identical(as.vector(NULL, modes[i]), expected[[i]]))
v <- structure(list(c(4L, 7L, 0L), integer()), class = "numeric_version",
               names = c("target", "missing"))
stopifnot(identical(format(v), c(target = "4.7.0", missing = NA_character_)),
          identical(as.character(v), c("4.7.0", NA_character_)),
          identical(as.character(structure(list(), class = "numeric_version")), character()))
empty <- character()
empty[logical()] <- NULL
stopifnot(identical(empty, character()))
cat("10 NULL modes; named, missing and empty versions; empty assignment: PASS\n")
x <- integer(); x[logical()] <- NULL; stopifnot(identical(x, integer()))
x <- integer(); x[logical()] <- 1.5; stopifnot(identical(x, double()))
x <- character(); x[logical()] <- 1L; stopifnot(identical(x, character()))
x <- NULL; x[2L] <- "z"; stopifnot(identical(x, c(NA_character_, "z")))
x <- character(); stopifnot(inherits(try(x[1L] <- NULL, silent = TRUE), "try-error"))
cat("NULL assignment, empty type promotion, growth and invalid replacement: PASS\n")
