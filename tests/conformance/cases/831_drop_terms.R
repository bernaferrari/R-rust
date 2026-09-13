t <- terms(y ~ x + z)
cat(deparse(drop.terms(t, 2)), "\n", sep = "")
cat(deparse(drop.terms(t, 1)), "\n", sep = "")
