set.seed(28)
n <- 80
s <- 4
e <- rnorm(n)
u <- numeric(n)
for (i in 1:n) {
  u[i] <- e[i]
  if (i > s) u[i] <- u[i] + 0.4 * u[i - s]
}
y <- numeric(n)
for (i in 1:n) {
  y[i] <- u[i]
  if (i > 1) y[i] <- y[i] + 0.3 * y[i - 1]
}
a <- arima(y, order = c(1, 0, 0), seasonal = list(order = c(1, 0, 0), period = 4), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["sar1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
