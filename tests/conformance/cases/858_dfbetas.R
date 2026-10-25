# Every leave-one-out fit has nonzero residual variance.
x <- 1:5
y <- c(1, 2, 2, 4, 6)
d <- dfbetas(lm(y ~ x))
cat(paste(round(as.vector(d), 4), collapse = ","), "\n", sep = "")
