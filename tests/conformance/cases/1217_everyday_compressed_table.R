compressed <- tempfile(fileext = ".tsv.gz")
export <- tempfile(fileext = ".csv")
# Standard gzip member for sample\tbatch\tvalue and rows 01/b/12, 02/a/NA, 03/a/8.
# Fixed bytes exercise the same file format users download and upload.
gzip <- as.raw(c(
    31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 43, 78, 204, 45, 200, 73, 229,
    76, 74, 44, 73, 206, 224, 44, 75, 204, 41, 77, 229, 50, 48, 228, 76,
    226, 52, 52, 226, 50, 48, 226, 76, 228, 244, 115, 228, 50, 48, 6,
    210, 22, 92, 0, 144, 57, 36, 37, 42, 0, 0, 0
))
writeBin(gzip, compressed)
types <- c("character", "character", "integer")
data <- read.delim(compressed, colClasses = types)
stopifnot(identical(data$sample, c("01", "02", "03")),
          identical(data$value, c(12L, NA_integer_, 8L)))
connection <- gzfile(compressed, "rt")
explicit <- read.delim(connection, colClasses = types)
close(connection)
stopifnot(identical(data, explicit))

clean <- data[!is.na(data$value), ]
write.csv(clean[order(clean$batch), ], export, row.names = FALSE)
round_trip <- read.csv(export, colClasses = types)
stopifnot(identical(round_trip$sample, c("03", "01")),
          identical(round_trip$batch, c("a", "b")),
          identical(round_trip$value, c(8L, 12L)))
unlink(c(compressed, export))
cat("Compressed tabular input and cleaned export agree\n")
