y <- 1:5
x <- 1:5
d <- dummy.coef(lm(y ~ x))
cat(d$`(Intercept)`, "\n", sep = "")
cat(d$x, "\n", sep = "")
cat(paste(names(d), collapse = ","), "\n", sep = "")
