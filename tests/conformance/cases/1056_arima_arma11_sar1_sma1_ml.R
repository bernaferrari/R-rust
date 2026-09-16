set.seed(38)
n <- 80
s <- 4
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
for (i in 2:n) {
  y[i] <- 0.3 * y[i - 1] + e[i] + 0.2 * e[i - 1]
  if (i > s) y[i] <- y[i] + 0.25 * y[i - s] + 0.15 * e[i - s]
}
a <- arima(y, order = c(1, 0, 1), seasonal = list(order = c(1, 0, 1), period = 4), method = "ML")
cat(abs(as.numeric(a$coef["ar1"]) - 0.604) < 0.01, "\n", sep = "")
cat(abs(as.numeric(a$coef["ma1"]) - (-0.233)) < 0.01, "\n", sep = "")
cat(abs(as.numeric(a$coef["sar1"]) - (-0.220)) < 0.01, "\n", sep = "")
cat(abs(as.numeric(a$coef["sma1"]) - 0.418) < 0.01, "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
