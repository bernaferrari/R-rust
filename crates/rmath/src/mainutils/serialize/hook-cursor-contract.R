local({
    for (token in c("token", "é", "a ", "\001", ""))
    for (version in 2:3) for (ascii in list(FALSE, TRUE, NA)) for (xdr in c(FALSE, TRUE)) {
        env <- new.env()
        hook <- function(x) token
        prefix <- serialize(list(env), NULL, ascii = ascii, xdr = xdr, version = version, refhook = hook)
        value <- c(list(env), as.list(1:64))
        bytes <- serialize(value, NULL, ascii = ascii, xdr = xdr, version = version, refhook = hook)
        con <- rawConnection(bytes, "rb")
        observed <- -1
        restored <- unserialize(con, refhook = function(x) { observed <<- seek(con); env })
        escaped <- token %in% c("é", "a ", "\001")
        expected <- length(prefix) + if ((isTRUE(ascii) || is.na(ascii)) && escaped) 1L else 0L
        if (observed != expected) stop(paste("hook cursor", token, version, ascii, xdr,
                                             "observed", observed, "expected", expected))
        stopifnot(identical(restored, value), seek(con) == length(bytes))
        close(con)
    }
    TRUE
})
