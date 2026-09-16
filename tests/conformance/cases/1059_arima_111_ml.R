set.seed(41)
n <- 80
e <- rnorm(n)
z <- numeric(n)
z[1] <- e[1]
for (i in 2:n) z[i] <- 0.3 * z[i - 1] + e[i] + 0.2 * e[i - 1]
y <- cumsum(z)
a <- arima(y, order = c(1, 1, 1), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.3f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(paste(names(a$coef), collapse = ","), "\n", sep = "")
