cat(paste(weights(list(weights = 1:3)), collapse = ","), "\n", sep = "")
cat(deparse(formula(list(formula = y ~ x))), "\n", sep = "")
cat(paste(terms(list(terms = 1:2)), collapse = ","), "\n", sep = "")
