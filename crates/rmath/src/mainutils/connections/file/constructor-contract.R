local({
    message <- function(expr) tryCatch({ force(expr); "accepted" }, error = conditionMessage)
    stopifnot(
        identical(message(file(c("a", "b"))), "invalid 'description' argument"),
        identical(message(file("deferred", blocking = NA)), "invalid 'blocking' argument"),
        identical(message(file(NA_character_, raw = NA)), "invalid 'description' argument"),
        identical(message(file("deferred", open = c("r", "w"))), "invalid 'open' argument"),
        identical(message(file("deferred", raw = NA)), "NA in coercion to boolean"),
        identical(message(file("deferred", raw = c(TRUE, FALSE))),
                  "'length = 2' in coercion to 'logical(1)'")
    )
    con <- file("deferred", blocking = "TRUE", raw = 0L)
    stopifnot(!isOpen(con), is.object(con), inherits(con, "connection"))
    close(con)
    con <- file("", open = "w+b", blocking = 1L, raw = 0L)
    stopifnot(isOpen(con, "r"), isOpen(con, "w"))
    invisible(serialize(7L, con))
    stopifnot(identical(unserialize(con), 7L))
    close(con)
    TRUE
})
