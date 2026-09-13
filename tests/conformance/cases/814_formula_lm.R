y <- c(1, 2, 2, 4, 5)
x <- 1:5
cat(deparse(formula(lm(y ~ x))), "\n", sep = "")
cat(deparse(formula(glm(y ~ x))), "\n", sep = "")
