set.seed(42)
n <- 80
s <- 4
e <- rnorm(n)
z <- e
for (i in 2:n) z[i] <- e[i] + 0.3 * e[i - 1]
yy <- z
for (i in (s + 1):n) yy[i] <- yy[i] + yy[i - s]
y <- cumsum(yy)
a <- arima(y, order = c(0, 1, 1), seasonal = list(order = c(0, 1, 1), period = 4), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["sma1"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(paste(names(a$coef), collapse = ","), "\n", sep = "")
