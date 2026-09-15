set.seed(4)
n <- 60
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > 1) y[i] <- y[i] + 0.4 * y[i - 1]
  if (i > s) y[i] <- y[i] + 0.5 * y[i - s]
}
a <- arima(y, order = c(1, 0, 0), seasonal = list(order = c(1, 0, 0), period = 4), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
