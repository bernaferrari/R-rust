set.seed(17)
n <- 80
s <- 4
e <- rnorm(n)
u <- numeric(n)
for (i in 1:n) {
  u[i] <- e[i]
  if (i > 1) u[i] <- u[i] + 0.4 * e[i - 1]
  if (i > s) u[i] <- u[i] + 0.3 * e[i - s]
}
y <- cumsum(u)
for (i in (s + 1):n) y[i] <- y[i] + y[i - s]
a <- arima(y, order = c(0, 1, 1), seasonal = list(order = c(0, 1, 1), period = 4), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
