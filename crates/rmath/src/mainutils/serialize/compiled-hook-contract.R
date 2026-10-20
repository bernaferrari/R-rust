local({
    con <- rawConnection(c(compiled, serialize(42L, NULL, version = 2)), "rb")
    seen <- numeric()
    restored <- unserialize(con, refhook = function(x) {
        seen <<- c(seen, seek(con))
        new.env()
    })
    stopifnot(identical(seen, c(1324, 3761)), is.function(restored),
              seek(con) == length(compiled), identical(unserialize(con), 42L))
    close(con)
    TRUE
})
