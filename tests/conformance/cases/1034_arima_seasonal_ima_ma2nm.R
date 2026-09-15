set.seed(16)
n <- 48
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > s) y[i] <- y[i] + y[i - s] + 0.35 * e[i - s]
}
a <- arima(y, order = c(0, 0, 1), seasonal = list(order = c(0, 1, 0), period = 4), method = "CSS")
cat(sprintf("%.3f", as.numeric(a$coef)), "\n", sep = "")
b <- arima(y, order = c(0, 0, 2), include.mean = FALSE, method = "CSS")
cat(paste(round(as.numeric(b$coef), 3), collapse = ","), "\n", sep = "")
