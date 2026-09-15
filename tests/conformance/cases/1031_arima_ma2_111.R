set.seed(12)
n <- 80
e <- rnorm(n)
y <- numeric(n)
for (i in 1:n) {
  y[i] <- e[i]
  if (i > 1) y[i] <- y[i] + 0.4 * e[i - 1]
  if (i > 2) y[i] <- y[i] + 0.25 * e[i - 2]
}
a <- arima(y, order = c(0, 0, 2), method = "CSS")
cat(paste(round(as.numeric(a$coef), 3), collapse = ","), "\n", sep = "")
z2 <- cumsum(y)
b <- arima(z2, order = c(1, 1, 1), method = "CSS")
cat(paste(round(as.numeric(b$coef), 3), collapse = ","), "\n", sep = "")
