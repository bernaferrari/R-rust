local({
    stopifnot(identical(getOption("useFancyQuotes"), TRUE),
              identical(sQuote("x"), sQuote("x", q = TRUE)),
              identical(dQuote("x"), dQuote("x", q = TRUE)))
    stopifnot(typeof(sQuote) == "closure", typeof(dQuote) == "closure",
              identical(names(formals(sQuote)), c("x", "q")),
              identical(names(formals(dQuote)), c("x", "q")))
    x <- c(alpha = "alpha", apostrophe = "O'Reilly", double = 'a"b',
           missing = NA_character_, empty = "")
    for (q in list(NULL, FALSE, TRUE, "UTF-8", "TeX", c("<", ">", "[", "]"))) {
        print(sQuote(x, q = q))
        print(dQuote(x, q = q))
    }
    for (x in list(NULL, character(), integer(), 1:3, c(TRUE, NA),
                   list("a", 2), factor(c("a", "b")))) {
        print(sQuote(x, q = FALSE))
        print(dQuote(x, q = FALSE))
    }
    calls <- 0L
    dQuote("once", q = { calls <- calls + 1L; FALSE })
    stopifnot(identical(calls, 1L))
    old <- options(useFancyQuotes = "TeX")
    on.exit(options(old), add = TRUE)
    stopifnot(identical(sQuote("x"), "`x'"), identical(dQuote("x"), "``x''"))
    options(useFancyQuotes = c("<", ">", "[", "]"))
    stopifnot(identical(sQuote("x"), "<x>"), identical(dQuote("x"), "[x]"))
    cat("quote formals/options/promise controls: TRUE\n")
})
