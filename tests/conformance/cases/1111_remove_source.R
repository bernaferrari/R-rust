f <- function(x) x + 1
b <- removeSource(body(f))
cat(paste(deparse(b), collapse = " "), "\n", sep = "")
g <- removeSource(f)
cat(is.function(g), "\n")
