set.seed(47)
n <- 80
e <- rnorm(n)
y <- numeric(n)
y[1] <- e[1]
for (i in 2:n) y[i] <- 0.4 * y[i - 1] + e[i] + 0.3 * e[i - 1]
a <- arima(y, order = c(1, 0, 1), include.mean = FALSE, method = "ML")
cat(sprintf("%.2f", as.numeric(a$coef["ar1"])), "\n", sep = "")
cat(sprintf("%.2f", as.numeric(a$coef["ma1"])), "\n", sep = "")
cat(sprintf("%.3f", a$sigma2), "\n", sep = "")
cat(paste(names(a$coef), collapse = ","), "\n", sep = "")
cat(length(a$coef), "\n", sep = "")
