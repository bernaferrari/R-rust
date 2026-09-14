d <- data.frame(y = 1:4, a = factor(c("x", "y", "x", "y")))
cat(paste(.getXlevels(terms(y ~ a), d)$a, collapse = ","), "\n", sep = "")
