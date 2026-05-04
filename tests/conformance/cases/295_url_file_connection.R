f <- tempfile()
writeChar("abcdef", f, eos = NULL)

u <- url(paste0("file://", f), "rb")
print(inherits(u, "connection"))
print(readChar(u, 3, useBytes = TRUE))
close(u)

invisible(unlink(f))
