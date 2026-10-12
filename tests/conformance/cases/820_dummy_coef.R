y <- 1:5
x <- 1:5
d <- dummy.coef(lm(y ~ x))
# The exact coefficients are 0 and 1; the QR residual arithmetic leaves
# a ~1e-16 contraction-dependent residual on some builds. round(..., 8)
# is the documented 5e-9 acceptance bound for these coefficients.
cat(paste(round(d$`(Intercept)`, 8), collapse = ","), "\n", sep = "")
cat(paste(round(d$x, 8), collapse = ","), "\n", sep = "")
cat(paste(names(d), collapse = ","), "\n", sep = "")
