options(digits=7, scipen=0)
fit <- lm(c(1, NA, 3, NaN, 5) ~ 1, na.action=na.exclude)
y <- covratio(fit)
# QR arithmetic can differ by a few rounding bits across architectures.
expected <- c(0.75, 3, 0.75)
stopifnot(identical(unname(is.na(y)), c(FALSE, TRUE, FALSE, TRUE, FALSE)),
          !any(is.nan(y)),
          all(abs(y[!is.na(y)] - expected) <= 8 * .Machine$double.eps * expected))
cat("default cat:"); cat(y); cat("\ndefault print:\n"); print(y)
cat("single cat:"); cat(y[1L]); cat("\nsingle print:\n"); print(unname(y[1L]))
x <- c(0.7500000000000006, -0.7500000000000006, NaN, NA_real_, 1.23456789, 1e-7, 1e7)
for (d in c(3, 4, 5)) for (s in c(-1, 0, 1)) {
  options(digits=d, scipen=s)
  cat("digits=", d, " scipen=", s, " cat="); cat(x, sep="|"); cat("\nprint:\n"); print(x)
}
