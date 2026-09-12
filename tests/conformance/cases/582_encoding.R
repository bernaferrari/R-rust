x <- "a"
cat(Encoding(x), "\n", sep = "")
Encoding(x) <- "UTF-8"
cat(Encoding(x), "\n", sep = "")
Encoding(x) <- "bytes"
cat(Encoding(x), "\n", sep = "")
Encoding(x) <- "latin1"
cat(Encoding(x), "\n", sep = "")
Encoding(x) <- "unknown"
cat(Encoding(x), "\n", sep = "")
y <- c("a", "b")
Encoding(y) <- c("UTF-8", "bytes")
cat(paste(Encoding(y), collapse = ","), "\n", sep = "")
