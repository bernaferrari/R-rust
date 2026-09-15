set.seed(23)
n <- 80
s <- 4
e <- rnorm(n)
u <- numeric(n)
for (i in 1:n) {
  u[i] <- e[i]
  if (i > 1) u[i] <- u[i] + 0.25 * u[i - 1] + 0.2 * e[i - 1]
  if (i > s) u[i] <- u[i] + 0.3 * u[i - s] + 0.25 * e[i - s]
}
y <- cumsum(u)
a <- arima(y, order = c(1, 1, 1), seasonal = list(order = c(1, 0, 1), period = 4), method = "CSS")
cat(paste(round(as.numeric(a$coef), 2), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
