set.seed(6)
n <- 40
s <- 4
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i] + (i %/% s) * 0.3
  if (i > s) y[i] <- y[i] + y[i - s]
}
a <- arima(y, order = c(0, 0, 0), seasonal = list(order = c(0, 1, 0), period = 4), method = "CSS")
cat(length(a$coef), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
b <- arima(y, order = c(1, 0, 0), seasonal = list(order = c(0, 1, 0), period = 4), method = "CSS")
cat(sprintf("%.3f", as.numeric(b$coef)), "\n", sep = "")
