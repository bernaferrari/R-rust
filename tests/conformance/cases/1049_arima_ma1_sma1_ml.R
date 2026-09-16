set.seed(30)
n <- 80
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > 1) y[i] <- y[i] + 0.3 * e[i - 1]
  if (i > s) y[i] <- y[i] + 0.4 * e[i - s]
  if (i > s + 1) y[i] <- y[i] + 0.12 * e[i - s - 1]
}
a <- arima(y, order = c(0, 0, 1), seasonal = list(order = c(0, 0, 1), period = 4), method = "ML")
cat(sprintf("%.2f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["sma1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
