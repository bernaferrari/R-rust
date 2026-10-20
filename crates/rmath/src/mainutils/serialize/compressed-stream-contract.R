local({
    con <- rawConnection(compressed, "rb")
    failure <- tryCatch(unserialize(con), error = conditionMessage)
    stopifnot(identical(failure, "unknown input format"), seek(con) == 2)
    close(con)
    con <- gzcon(rawConnection(compressed, "rb"))
    stopifnot(identical(unserialize(con), 7L), identical(unserialize(con), 9L),
              identical(readBin(con, "raw", n = 2L), as.raw(c(71L, 72L))))
    close(con)
    TRUE
})
