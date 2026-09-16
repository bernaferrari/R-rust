set.seed(44)
n <- 90
e <- rnorm(n)
z <- numeric(n)
z[1] <- e[1]
z[2] <- 0.4 * z[1] + e[2]
for (i in 3:n) z[i] <- 0.4 * z[i - 1] - 0.25 * z[i - 2] + e[i]
y <- cumsum(z)
a <- arima(y, order = c(2, 1, 0), method = "ML")
cat(sprintf("%.2f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["ar2"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(paste(names(a$coef), collapse = ","), "\n", sep = "")
