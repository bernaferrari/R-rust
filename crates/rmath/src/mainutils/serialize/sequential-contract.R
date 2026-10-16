local({
    count <- 0L
    for (version in 2:3) for (ascii in list(FALSE, TRUE, NA)) for (xdr in c(FALSE, TRUE)) {
        value <- list(1L, "text", c(TRUE, NA))
        a <- serialize(value, NULL, ascii = ascii, xdr = xdr, version = version)
        b <- serialize(42L, NULL, ascii = ascii, xdr = xdr, version = version)
        tail <- as.raw(c(71L, 72L))
        for (kind in c("raw", "anonymous")) {
            tryCatch({
            if (kind == "raw") con <- rawConnection(c(a, b, tail), "rb") else {
                con <- file("", "w+b")
                writeBin(c(a, b, tail), con)
                seek(con, 0, rw = "read")
            }
            stopifnot(identical(unserialize(con), value), seek(con) == length(a),
                      identical(unserialize(con), 42L), seek(con) == length(a) + length(b),
                      identical(readBin(con, "raw", n = 2L), tail))
            close(con)
            count <- count + 1L
            }, error = function(e) stop(paste(version, ascii, xdr, kind, conditionMessage(e))))
        }
    }
    stopifnot(count == 24L)
    TRUE
})
