set.seed(18)
n <- 80
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
if (n >= 2) y[2] <- 0.4 * y[1] + e[2]
if (n >= 3) y[3] <- 0.4 * y[2] - 0.2 * y[1] + e[3]
for (i in 4:n) y[i] <- 0.4 * y[i - 1] - 0.2 * y[i - 2] + 0.15 * y[i - 3] + e[i]
a <- arima(y, order = c(3, 0, 0), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
