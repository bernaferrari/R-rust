S <- matrix(c(1, 0.5, 0.5, 1), 2, 2)
x <- cbind(c(1, 2, 3), c(2, 3, 5))
cat(paste(round(mahalanobis(x, c(2, 3), S), 4), collapse = ","), "\n", sep = "")
