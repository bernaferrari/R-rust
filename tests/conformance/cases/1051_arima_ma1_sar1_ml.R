set.seed(32)
n <- 80
s <- 4
e <- rnorm(n)
u <- numeric(n)
for (i in 1:n) {
  u[i] <- e[i]
  if (i > 1) u[i] <- u[i] + 0.3 * e[i - 1]
}
y <- numeric(n)
for (i in 1:n) {
  y[i] <- u[i]
  if (i > s) y[i] <- y[i] + 0.4 * y[i - s]
}
a <- arima(y, order = c(0, 0, 1), seasonal = list(order = c(1, 0, 0), period = 4), method = "ML")
cat(abs(as.numeric(a$coef["ma1"]) - 0.274) < 0.01, "\n", sep = "")
cat(abs(as.numeric(a$coef["sar1"]) - 0.542) < 0.02, "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(abs(a$sigma2 - 0.675) < 0.002, "\n", sep = "")
