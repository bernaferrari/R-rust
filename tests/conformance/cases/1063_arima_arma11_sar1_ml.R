set.seed(45)
n <- 80
s <- 4
e <- rnorm(n)
u <- numeric(n)
u[1] <- e[1]
for (i in 2:n) u[i] <- 0.3 * u[i - 1] + e[i] + 0.2 * e[i - 1]
y <- numeric(n)
for (i in 1:n) {
  y[i] <- u[i]
  if (i > s) y[i] <- y[i] + 0.35 * y[i - s]
}
a <- arima(y, order = c(1, 0, 1), seasonal = list(order = c(1, 0, 0), period = 4), method = "ML")
cat(sprintf("%.2f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["sar1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
