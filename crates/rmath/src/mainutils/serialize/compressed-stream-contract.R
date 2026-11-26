local({
    rawstream <- c(serialize(7L, NULL), serialize(9L, NULL), as.raw(c(71L, 72L)))
    compressed <- memCompress(rawstream, "gzip")
    con <- rawConnection(compressed, "rb")
    failure <- tryCatch(unserialize(con), error = conditionMessage)
    # Compressed input read without gzcon is garbage to the format layer and
    # must error. The exact number of bytes consumed before the error is an
    # implementation detail; the stream must not rewind or advance past the
    # input end.
    stopifnot(identical(failure, "not an RBG1 serialization stream"),
              seek(con) > 0, seek(con) <= length(compressed))
    close(con)
    con <- gzcon(rawConnection(compressed, "rb"))
    stopifnot(identical(unserialize(con), 7L), identical(unserialize(con), 9L),
              identical(readBin(con, "raw", n = 2L), as.raw(c(71L, 72L))))
    close(con)
    TRUE
})
