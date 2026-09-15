set.seed(18)
n <- 80
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
y[2] <- 0.4 * y[1] + e[2]
for (i in 3:n) y[i] <- 0.4 * y[i - 1] - 0.2 * y[i - 2] + e[i]
a <- arima(y, order = c(2, 0, 0), include.mean = FALSE, method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(length(a$coef), "\n", sep = "")
