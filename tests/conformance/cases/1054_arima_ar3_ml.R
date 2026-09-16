set.seed(35)
n <- 90
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
y[2] <- 0.3 * y[1] + e[2]
y[3] <- 0.3 * y[2] - 0.2 * y[1] + e[3]
for (i in 4:n) y[i] <- 0.3 * y[i - 1] - 0.2 * y[i - 2] + 0.15 * y[i - 3] + e[i]
a <- arima(y, order = c(3, 0, 0), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["ar2"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["ar3"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
