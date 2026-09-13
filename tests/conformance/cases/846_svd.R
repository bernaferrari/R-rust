s <- svd(matrix(c(1, 2, 3, 4), 2, 2))
cat(paste(round(s$d, 4), collapse = ","), "\n", sep = "")
cat(length(s$d), "\n", sep = "")
