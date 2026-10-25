# Every leave-one-out fit has nonzero residual variance.
y <- c(1, 2, 2, 4, 6)
x <- 1:5
cr <- covratio(lm(y ~ x))
cat(paste(round(cr, 4), collapse = ","), "\n", sep = "")
