y <- c(1, 2, 2, 4, 5)
x <- 1:5
u <- update(lm(y ~ x))
cat(deparse(getCall(u)[[2]]), "\n", sep = "")
cat(paste(round(as.numeric(coef(u)), 4), collapse = ","), "\n", sep = "")
cat(class(u), "\n", sep = "")
