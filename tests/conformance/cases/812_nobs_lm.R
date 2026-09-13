y <- c(1, 2, 2, 4, 5)
x <- 1:5
cat(nobs(lm(y ~ x)), "\n", sep = "")
cat(nobs(glm(y ~ x)), "\n", sep = "")
