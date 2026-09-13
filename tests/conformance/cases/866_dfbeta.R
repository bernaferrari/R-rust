x <- 1:5
y <- c(1, 2, 2, 4, 5)
d <- dfbeta(lm(y ~ x))
cat(paste(round(as.vector(d), 4), collapse = ","), "\n", sep = "")
