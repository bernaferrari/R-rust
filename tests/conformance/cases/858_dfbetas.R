x <- 1:5
y <- c(1, 2, 2, 4, 5)
d <- as.vector(dfbetas(lm(y ~ x)))
d[is.nan(d) | abs(d) > 1e6] <- 0
cat(paste(round(d, 4), collapse = ","), "\n", sep = "")
