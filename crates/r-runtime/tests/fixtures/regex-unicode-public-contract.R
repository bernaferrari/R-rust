local({
    text <- "aéλé"
    for (perl in c(FALSE, TRUE)) {
        first <- regexpr("é", text, perl = perl)
        stopifnot(identical(first, structure(2L, match.length = 1L)))
        bytes <- regexpr("é", text, perl = perl, useBytes = TRUE)
        stopifnot(identical(bytes, structure(2L, match.length = 2L, index.type = "bytes", useBytes = TRUE)))
        repeated <- gregexpr("é", text, perl = perl)[[1L]]
        stopifnot(identical(repeated, structure(c(2L, 4L), match.length = c(1L, 1L))))
        repeated_bytes <- gregexpr("é", text, perl = perl, useBytes = TRUE)[[1L]]
        stopifnot(identical(repeated_bytes, structure(c(2L, 6L), match.length = c(2L, 2L), index.type = "bytes", useBytes = TRUE)))
        missing <- gregexpr("é", c(text, NA_character_), perl = perl)[[2L]]
        stopifnot(identical(missing, structure(NA_integer_, match.length = NA_integer_)))
    }
    for (perl in c(FALSE, TRUE)) {
        dots <- gregexpr(".", "éλ", perl = perl)[[1L]]
        stopifnot(identical(as.integer(dots), c(1L, 2L)), identical(attr(dots, "match.length"), c(1L, 1L)))
        dots_bytes <- gregexpr(".", "éλ", perl = perl, useBytes = TRUE)[[1L]]
        stopifnot(identical(as.integer(dots_bytes), 1:4), identical(attr(dots_bytes, "match.length"), rep(1L, 4)))
        stopifnot(identical(as.integer(regexpr(".{2}", "éλ", perl = perl)), 1L),
                  identical(attr(regexpr(".{2}", "éλ", perl = perl), "match.length"), 2L))
    }
    groups <- regexec("(é)", text, perl = TRUE)[[1L]]
    stopifnot(identical(groups, structure(c(2L, 2L), match.length = c(1L, 1L))))
    stopifnot(identical(as.integer(gregexpr("^.", "abc", perl = TRUE)[[1L]]), 1L),
              identical(as.integer(gregexpr("(?<=a)b", "abab", perl = TRUE)[[1L]]), c(2L, 4L)),
              identical(gsub("(?<=a)b", "X", "abab", perl = TRUE), "aXaX"),
              identical(gsub("\\ba", "X", "aa aa", perl = TRUE), "Xa Xa"))
    for (fun in list(grepl, grep, regexpr, gregexpr, regexec)) {
        message <- suppressWarnings(tryCatch(fun("[", "ab", perl = TRUE), error = function(e) conditionMessage(e)))
        stopifnot(identical(message, "invalid regular expression '['"))
    }
    for (fun in list(regexpr, gregexpr, regexec)) {
        message <- tryCatch(fun(NA_character_, "abc"), error = function(e) conditionMessage(e))
        stopifnot(identical(message, "invalid 'pattern' argument"))
    }
    stopifnot(identical(gsub("a*", "X", "abc", perl = TRUE), "XbXcX"),
              identical(gsub("a*", "X", "a", perl = TRUE), "X"))
    TRUE
})
