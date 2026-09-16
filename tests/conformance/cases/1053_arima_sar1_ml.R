set.seed(34)
n <- 80
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > s) y[i] <- y[i] + 0.4 * y[i - s]
}
a <- arima(y, order = c(0, 0, 0), seasonal = list(order = c(1, 0, 0), period = 4), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["sar1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
