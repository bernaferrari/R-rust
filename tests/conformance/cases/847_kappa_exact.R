S <- matrix(c(4, 2, 2, 3), 2, 2)
cat(round(kappa(S, exact = TRUE), 4), "\n", sep = "")
cat(round(kappa(S, method = "direct"), 4), "\n", sep = "")
