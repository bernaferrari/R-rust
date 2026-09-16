set.seed(1)
x <- rnorm(20)
q <- quantile(x, 0.95, names = FALSE)
cat(is.null(names(q)), "\n", sep = "")
cat(sprintf("%.6f", q), "\n", sep = "")
Meps <- .Machine$double.eps
rErr <- function(approx, true, eps = 1e-30) {
  ifelse(Mod(true) >= eps, 1 - approx / true, true - approx)
}
n <- 1031
y <- rnorm(n)
er <- Mod(rErr(fft(fft(y), inverse = TRUE) / n, y * (1 + 0i)))
cat(all(er < 1e-8), "\n", sep = "")
cat(quantile(er, 0.95, names = FALSE) < 10000 * Meps, "\n", sep = "")
