y <- c(1, 2, 2, 4, 5)
x <- 1:5
s <- step(lm(y ~ x), trace = 0)
cat(deparse(formula(s)), "\n", sep = "")
cat(paste(round(as.numeric(coef(s)), 4), collapse = ","), "\n", sep = "")
cat(class(s), "\n", sep = "")
