R <- chol(matrix(c(4, 2, 2, 3), 2, 2))
cat(paste(round(backsolve(R, c(1, 1)), 4), collapse = ","), "\n", sep = "")
cat(paste(round(forwardsolve(t(R), c(1, 1)), 4), collapse = ","), "\n", sep = "")
