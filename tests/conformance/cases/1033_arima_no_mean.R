set.seed(15)
n <- 60
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
for (i in 2:n) y[i] <- 0.5 * y[i - 1] + e[i]
a <- arima(y, order = c(1, 0, 0), include.mean = FALSE, method = "CSS")
cat(sprintf("%.3f", as.numeric(a$coef)), "\n", sep = "")
b <- arima(y, order = c(0, 0, 1), include.mean = FALSE, method = "CSS")
cat(sprintf("%.2f", as.numeric(b$coef)), "\n", sep = "")
