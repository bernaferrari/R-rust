f <- tempfile()
writeLines(c("a", "b"), con = f, sep = "|")
print(readLines(f, warn = FALSE))
g <- tempfile()
writeLines(c("a", "b"), g)
print(readLines(g, warn = FALSE))

