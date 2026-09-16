set.seed(29)
n <- 80
s <- 4
e <- rnorm(n)
y <- e
for (i in (s + 1):n) y[i] <- e[i] + 0.4 * e[i - s]
a <- arima(y, order = c(0, 0, 0), seasonal = list(order = c(0, 0, 1), period = 4), method = "ML")
cat(sprintf("%.2f", as.numeric(a$coef["sma1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
