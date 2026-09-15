set.seed(22)
n <- 80
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > 1) y[i] <- y[i] + 0.25 * y[i - 1] + 0.2 * e[i - 1]
  if (i > s) y[i] <- y[i] + 0.3 * y[i - s] + 0.25 * e[i - s]
}
a <- arima(y, order = c(1, 0, 1), seasonal = list(order = c(1, 0, 1), period = 4), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
