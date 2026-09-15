set.seed(19)
n <- 72
s <- 4
e <- rnorm(n)
u <- numeric(n)
for (i in 1:n) {
  u[i] <- e[i]
  if (i > 1) u[i] <- u[i] + 0.35 * u[i - 1]
  if (i > s) u[i] <- u[i] + 0.4 * u[i - s]
}
y <- cumsum(u)
a <- arima(y, order = c(1, 1, 0), seasonal = list(order = c(1, 0, 0), period = 4), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
