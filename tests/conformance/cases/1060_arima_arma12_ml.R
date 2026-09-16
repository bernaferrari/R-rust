set.seed(43)
n <- 90
e <- rnorm(n)
y <- e
for (i in 3:n) y[i] <- 0.3 * y[i - 1] + e[i] + 0.4 * e[i - 1] + 0.2 * e[i - 2]
a <- arima(y, order = c(1, 0, 2), method = "ML")
cat(abs(as.numeric(a$coef["ar1"]) - 0.372) < 0.02, "\n", sep = "")
cat(abs(as.numeric(a$coef["ma1"]) - 0.479) < 0.02, "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["ma2"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
