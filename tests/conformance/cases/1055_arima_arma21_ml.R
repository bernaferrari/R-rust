set.seed(37)
n <- 120
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
y[2] <- 0.5 * y[1] + e[2] - 0.4 * e[1]
for (i in 3:n) y[i] <- 0.5 * y[i - 1] - 0.3 * y[i - 2] + e[i] - 0.4 * e[i - 1]
a <- arima(y, order = c(2, 0, 1), method = "ML")
cat(abs(as.numeric(a$coef["ar1"]) - 0.466) < 0.06, "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["ar2"])), "\n", sep = "")
cat(abs(as.numeric(a$coef["ma1"]) - (-0.393)) < 0.06, "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
