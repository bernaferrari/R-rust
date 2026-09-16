set.seed(40)
n <- 80
e <- rnorm(n)
z <- e
for (i in 2:n) z[i] <- e[i] + 0.4 * e[i - 1]
y <- cumsum(z)
a <- arima(y, order = c(0, 1, 1), method = "ML")
cat(sprintf("%.3f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(names(a$coef)[1], "\n", sep = "")
cat(length(a$coef), "\n", sep = "")
