for (bytes in c(FALSE, TRUE)) {
    for (replacement in c("x", NA_character_)) {
        stopifnot(identical(tryCatch(sub("", replacement, c("abc", ""), fixed = TRUE, useBytes = bytes), error = function(e) conditionMessage(e)), "zero-length pattern"))
        stopifnot(identical(tryCatch(gsub("", replacement, c("abc", ""), fixed = TRUE, useBytes = bytes), error = function(e) conditionMessage(e)), "zero-length pattern"))
    }
}
stopifnot(identical(tryCatch(sub("", NA_character_, character(), fixed = TRUE), error = function(e) conditionMessage(e)), "zero-length pattern"))
stopifnot(identical(grepl("", c("abc", ""), fixed = TRUE), c(TRUE, TRUE)))
stopifnot(identical(sub("", "x", c("abc", "")), c("xabc", "x")))
stopifnot(identical(gsub("", "x", c("abc", "")), c("xaxbxcx", "x")))
TRUE
