set.seed(27)
n <- 80
e <- rnorm(n)
y <- e
for (i in 3:n) y[i] <- e[i] + 0.4 * e[i - 1] + 0.2 * e[i - 2]
a <- arima(y, order = c(0, 0, 2), method = "ML")
cat(sprintf("%.2f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["ma2"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
