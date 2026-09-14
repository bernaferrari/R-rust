S <- matrix(c(1, 1.5, 1.5, 2.333333), 2, 2)
cat(paste(round(as.vector(cov2cor(S)), 4), collapse = ","), "\n", sep = "")
