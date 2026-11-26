local({
    env <- new.env()
    f <- cmpfun(function(x) x + 1L)
    environment(f) <- env
    compiled <- serialize(f, NULL, refhook = function(x) "token")
    con <- rawConnection(c(compiled, serialize(42L, NULL)), "rb")
    seen <- numeric()
    restored <- unserialize(con, refhook = function(x) {
        seen <<- c(seen, seek(con))
        env
    })
    # RBG1 frames are atomic: the full frame is consumed before materialization
    # begins, so every restoration hook observes the end-of-frame cursor.
    stopifnot(identical(seen, as.numeric(length(compiled))), is.function(restored),
              seek(con) == length(compiled), identical(unserialize(con), 42L))
    close(con)
    TRUE
})
