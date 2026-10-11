y <- 1:5
x <- 1:5
d <- dummy.coef(lm(y ~ x))
cat(paste(round(d$`(Intercept)`, 8), collapse = ","), "\n", sep = "")
cat(paste(round(d$x, 8), collapse = ","), "\n", sep = "")
cat(paste(names(d), collapse = ","), "\n", sep = "")
