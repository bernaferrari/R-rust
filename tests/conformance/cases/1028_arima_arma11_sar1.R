set.seed(9)
n <- 72
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > 1) y[i] <- y[i] + 0.35 * y[i - 1] + 0.25 * e[i - 1]
  if (i > s) y[i] <- y[i] + 0.45 * y[i - s]
}
a <- arima(y, order = c(1, 0, 1), seasonal = list(order = c(1, 0, 0), period = 4), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
