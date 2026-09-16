set.seed(48)
n <- 100
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
y[2] <- 0.4 * y[1] + e[2] - 0.3 * e[1]
for (i in 3:n) y[i] <- 0.4 * y[i - 1] - 0.2 * y[i - 2] + e[i] - 0.3 * e[i - 1]
a <- arima(y, order = c(2, 0, 1), include.mean = FALSE, method = "ML")
cat(abs(as.numeric(a$coef["ar1"]) - 0.377) < 0.05, "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["ar2"])), "\n", sep = "")
cat(abs(as.numeric(a$coef["ma1"]) - (-0.294)) < 0.05, "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(paste(names(a$coef), collapse = ","), "\n", sep = "")
