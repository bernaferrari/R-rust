S <- matrix(c(4, 2, 2, 3), 2, 2)
cat(round(kappa(S, method = "direct"), 4), "\n", sep = "")
cat(round(1 / rcond(S), 4), "\n", sep = "")
cat(kappa(diag(2)), "\n", sep = "")
