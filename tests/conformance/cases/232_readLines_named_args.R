f <- tempfile()
cat("alpha", file = f)
print(readLines(f, warn = FALSE))
cat("beta\ngamma\n", file = f)
print(readLines(f, n = 1, warn = FALSE))

