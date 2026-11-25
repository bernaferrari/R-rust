local({
    for (token in c("token", "é", "a ", "\001", ""))
    for (version in 2:3) for (ascii in list(FALSE, TRUE, NA)) for (xdr in c(FALSE, TRUE)) {
        env <- new.env()
        hook <- function(x) token
        value <- c(list(env), as.list(1:64))
        bytes <- serialize(value, NULL, ascii = ascii, xdr = xdr, version = version, refhook = hook)
        con <- rawConnection(bytes, "rb")
        observed <- -1
        restored <- unserialize(con, refhook = function(x) { observed <<- seek(con); env })
        # RBG1 frames are atomic: the full frame is consumed from the stream
        # before materialization begins, so a restoration hook observes the
        # end-of-frame cursor rather than a mid-stream position.
        expected <- length(bytes)
        if (observed != expected) stop(paste("hook cursor", token, version, ascii, xdr,
                                             "observed", observed, "expected", expected))
        stopifnot(identical(restored, value), seek(con) == length(bytes))
        close(con)
    }
    TRUE
})
