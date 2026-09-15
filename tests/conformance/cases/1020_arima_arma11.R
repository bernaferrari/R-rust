set.seed(1)
n <- 80
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
for (i in 2:n) y[i] <- 0.5 * y[i - 1] + e[i] + 0.4 * e[i - 1]
a <- arima(y, order = c(1, 0, 1), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
