cat(sink.number(), "\n", sep = "")
f <- tempfile()

sink(f)
cat("alpha", "beta", sep = "|")
print(1)
sink()

cat(sink.number(), "\n", sep = "")
cat(paste(readLines(f, warn = FALSE), collapse = "\n"), "\n", sep = "")
invisible(unlink(f))
