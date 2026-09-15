set.seed(14)
n <- 80
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
for (i in 2:n) y[i] <- y[i - 1] + e[i] + 0.4 * e[i - 1]
a <- arima(y, order = c(0, 1, 1), method = "CSS")
cat(sprintf("%.3f", as.numeric(a$coef)), "\n", sep = "")
b <- arima(y, order = c(2, 1, 0), method = "CSS")
cat(paste(round(as.numeric(b$coef), 3), collapse = ","), "\n", sep = "")
