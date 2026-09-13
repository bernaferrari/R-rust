y <- c(1, 2, 2, 4, 5)
x <- 1:5
f <- family(glm(y ~ x))
cat(f$family, "\n", sep = "")
cat(f$link, "\n", sep = "")
cat(class(f), "\n", sep = "")
