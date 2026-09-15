set.seed(7)
n <- 36
s <- 4
e <- rnorm(n)
y <- cumsum(e)
for (i in (s + 1):n) y[i] <- y[i] + 0.4 * y[i - s]
a <- arima(y, order = c(0, 1, 0), seasonal = list(order = c(0, 1, 0), period = 4), method = "CSS")
cat(length(a$coef), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
b <- arima(y, order = c(1, 1, 0), seasonal = list(order = c(0, 1, 0), period = 4), method = "CSS")
cat(sprintf("%.3f", as.numeric(b$coef)), "\n", sep = "")
