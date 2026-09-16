set.seed(26)
n <- 80
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
y[2] <- 0.4 * y[1] + e[2]
for (i in 3:n) y[i] <- 0.4 * y[i - 1] - 0.25 * y[i - 2] + e[i]
a <- arima(y, order = c(2, 0, 0), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["ar2"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
