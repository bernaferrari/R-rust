f <- tempfile()
con <- file(f, "wb")
writeChar("abcdef", con, eos = NULL)
close(con)

con <- file(f, "rb")
print(readChar(con, 3, useBytes = TRUE))
print(readChar(con, 3, useBytes = TRUE))
close(con)

print(readChar(f, 2, useBytes = TRUE))

con <- file(f, "wb")
writeChar("abcdef", con, nchars = 4, eos = NULL)
close(con)
print(readChar(f, 10, useBytes = TRUE))

invisible(unlink(f))
