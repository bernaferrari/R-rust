S <- matrix(c(4, 2, 2, 3), 2, 2)
cat(round(rcond(S), 4), "\n", sep = "")
cat(round(rcond(S, "I"), 4), "\n", sep = "")
cat(rcond(diag(2)), "\n", sep = "")
