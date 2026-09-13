x <- 1:10
y <- 1 - exp(-0.4 * x) + c(0.01, -0.01, 0, 0.02, -0.01, 0, 0.01, -0.02, 0, 0.01)
f <- nls(y ~ 1 - exp(-b * x), start = list(b = 0.3))
cat(round(unname(coef(f)), 6), "\n", sep = "")
cat(class(f), "\n", sep = "")
