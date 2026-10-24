local({
  x <- setNames(c("é", "βa", NA_character_, ""), c("un", "deux", "trois", "quatre"))
  for (perl in c(FALSE, TRUE)) {
    stopifnot(identical(grep("é", x, fixed=TRUE), 1L))
    stopifnot(identical(grep("é", x, fixed=TRUE, value=TRUE), x[1L]))
    stopifnot(identical(grep("é", x, fixed=TRUE, invert=TRUE), 2:4))
    stopifnot(identical(grep("é", x, fixed=TRUE, invert=TRUE, value=TRUE), x[2:4]))
    stopifnot(identical(grep("^$", x, perl=perl, value=TRUE), x[4L]))
    stopifnot(identical(grep("β", x, perl=perl, value=TRUE), x[2L]))
    stopifnot(identical(grep("missing", x, perl=perl, value=TRUE), setNames(character(), character())))
    stopifnot(identical(grep("é", character(), perl=perl), integer()))
  }
  stopifnot(identical(grep("\\w", x, perl=TRUE), 2L))
  stopifnot(identical(grep("\\w", x, perl=TRUE, value=TRUE), x[2L]))
  stopifnot(identical(grep("1", c(1L, 12L, 2L), fixed=TRUE, value=TRUE), c("1", "12")))
  cat("grep-owned-index-contract\n")
})
