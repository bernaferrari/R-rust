x <- matrix(c(1, 2, 3, 2, 3, 4, 3, 4, 6, 5, 6, 8), 4, 3)
f <- factanal(x, factors = 1)
cat(paste(round(f$uniquenesses, 4), collapse = ","), "\n", sep = "")
cat(paste(round(as.vector(f$loadings), 4), collapse = ","), "\n", sep = "")
