set.seed(25)
n <- 70
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
for (i in 2:n) y[i] <- 0.4 * y[i - 1] + e[i] + 0.3 * e[i - 1]
a <- arima(y, order = c(1, 0, 1), method = "ML")
cat(sprintf("%.2f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
