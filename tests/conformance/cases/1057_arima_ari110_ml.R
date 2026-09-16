set.seed(39)
n <- 80
e <- rnorm(n)
z <- numeric(n)
z[1] <- e[1]
for (i in 2:n) z[i] <- 0.4 * z[i - 1] + e[i]
y <- cumsum(z)
a <- arima(y, order = c(1, 1, 0), method = "ML")
cat(abs(as.numeric(a$coef["ar1"]) - 0.651) < 0.01, "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(names(a$coef)[1], "\n", sep = "")
cat(length(a$coef), "\n", sep = "")
