patterns <- c("[", "(abc", "abc\\", "[z-a]", "[[:notaclass:]]", "a{3,1}")
for (pattern in patterns) {
    for (operation in c("grepl", "sub", "regexpr")) {
        caught <- tryCatch({
            if (operation == "grepl") grepl(pattern, "abc")
            else if (operation == "sub") sub(pattern, "x", "abc")
            else regexpr(pattern, "abc")
            FALSE
        }, error = function(e) grepl("invalid regular expression", conditionMessage(e), fixed = TRUE))
        stopifnot(isTRUE(caught))
        stopifnot(identical(grepl("[a-c]+", "abc"), TRUE))
        stopifnot(identical(1L + 1L, 2L))
    }
}
TRUE
