set.seed(31)
n <- 80
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > 1) y[i] <- y[i] + 0.3 * y[i - 1]
  if (i > s) y[i] <- y[i] + 0.4 * e[i - s]
}
a <- arima(y, order = c(1, 0, 0), seasonal = list(order = c(0, 0, 1), period = 4), method = "ML")
cat(abs(as.numeric(a$coef["ar1"]) - 0.455) < 0.01, "\n", sep = "")
cat(abs(as.numeric(a$coef["sma1"]) - 0.208) < 0.01, "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["intercept"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
