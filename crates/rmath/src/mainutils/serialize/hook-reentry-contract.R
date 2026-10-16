local({
    for (ascii in list(FALSE, TRUE)) {
        env <- new.env()
        a <- serialize(env, NULL, ascii = ascii, refhook = function(x) "token")
        b <- serialize(42L, NULL, ascii = ascii)
        con <- rawConnection(c(a, b), "rb")
        nested <- NULL
        restored <- unserialize(con, refhook = function(x) { nested <<- unserialize(con); env })
        stopifnot(identical(restored, env), identical(nested, 42L), seek(con) == length(a) + length(b))
        close(con)
        con <- rawConnection(a, "rb")
        # A hook may close a fully consumed stream: no further I/O is needed.
        restored <- unserialize(con, refhook = function(x) { close(con); env })
        stopifnot(identical(restored, env))
    }
    TRUE
})
