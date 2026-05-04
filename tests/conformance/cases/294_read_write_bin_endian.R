f <- tempfile()
con <- file(f, "wb")
writeBin(as.integer(c(1L, -2L, 16777216L)), con, size = 4, endian = "little")
writeBin(c(1.5, -2.25), con, size = 8, endian = "little")
writeBin(as.raw(c(65, 66, 0, 67)), con)
close(con)

con <- file(f, "rb")
cat(paste(readBin(con, integer(), 3, size = 4, endian = "little"), collapse = ","), "\n", sep = "")
cat(paste(readBin(con, numeric(), 2, size = 8, endian = "little"), collapse = ","), "\n", sep = "")
print(readBin(con, raw(), 4))
close(con)

cat(
    paste(
        readBin(
            as.raw(c(1, 0, 0, 0, 255, 255, 255, 255)),
            integer(),
            2,
            size = 4,
            signed = TRUE,
            endian = "little"
        ),
        collapse = ","
    ),
    "\n",
    sep = ""
)
print(writeBin(as.integer(1), raw(), size = 4, endian = "little"))

invisible(unlink(f))
