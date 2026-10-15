# Generate with the pinned GNU R oracle, independently of the Rust encoder.
args <- commandArgs(TRUE)
if (length(args) != 1L) stop("expected output TSV path")
payloads <- list(empty = raw(), text = charToRaw(enc2utf8("R belongs everywhere.\n\u00e9\n")),
                 bytes = as.raw(0:255),
                 window = as.raw(((0:99999) * 17 + (0:99999) %/% 256) %% 256))
rows <- character()
for (kind in names(payloads)) {
    for (level in if (kind == "window") c(1L, 6L, 9L) else 0:9) {
        path <- tempfile()
        connection <- gzfile(path, "wb", compression = level)
        writeBin(payloads[[kind]], connection)
        close(connection)
        bytes <- readBin(path, "raw", n = file.info(path)$size)
        unlink(path)
        rows <- c(rows, paste(kind, level, paste(sprintf("%02x", as.integer(bytes)), collapse = ""), sep = "\t"))
    }
}
writeLines(rows, args[[1]])
