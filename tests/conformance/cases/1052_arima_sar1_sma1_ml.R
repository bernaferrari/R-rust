set.seed(33)
n <- 80
s <- 4
e <- rnorm(n)
u <- numeric(n)
for (i in 1:n) {
  u[i] <- e[i]
  if (i > s) u[i] <- u[i] + 0.4 * e[i - s]
}
y <- numeric(n)
for (i in 1:n) {
  y[i] <- u[i]
  if (i > s) y[i] <- y[i] + 0.3 * y[i - s]
}
a <- arima(y, order = c(0, 0, 0), seasonal = list(order = c(1, 0, 1), period = 4), method = "ML")
cat(abs(as.numeric(a$coef["sar1"]) - 0.363) < 0.01, "\n", sep = "")
cat(abs(as.numeric(a$coef["sma1"]) - 0.518) < 0.01, "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
