set.seed(11)
n <- 64
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > s) y[i] <- y[i] + 0.45 * y[i - s] + 0.3 * e[i - s]
}
a <- arima(y, order = c(0, 0, 0), seasonal = list(order = c(1, 0, 1), period = 4), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
