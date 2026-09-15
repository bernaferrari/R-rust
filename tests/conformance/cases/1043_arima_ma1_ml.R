set.seed(24)
n <- 60
e <- rnorm(n)
y <- e
for (i in 2:n) y[i] <- e[i] + 0.4 * e[i - 1]
a <- arima(y, order = c(0, 0, 1), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(abs(as.numeric(a$coef["ma1"]) - 0.536) < 0.01, "\n", sep = "")
cat(names(a$coef)[1], "\n", sep = "")
