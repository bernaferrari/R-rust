local({
    for (version in 2:3) for (ascii in list(FALSE, TRUE, NA)) for (xdr in c(FALSE, TRUE)) {
        env <- new.env()
        hook <- function(x) "token"
        prefix <- serialize(list(env), NULL, ascii = ascii, xdr = xdr, version = version, refhook = hook)
        value <- c(list(env), as.list(1:64))
        bytes <- serialize(value, NULL, ascii = ascii, xdr = xdr, version = version, refhook = hook)
        con <- rawConnection(bytes, "rb")
        observed <- -1
        restored <- unserialize(con, refhook = function(x) { observed <<- seek(con); env })
        expected <- length(prefix)
        stopifnot(observed == expected, identical(restored, value), seek(con) == length(bytes))
        close(con)
    }
    TRUE
})
