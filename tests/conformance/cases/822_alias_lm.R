y <- 1:5
x <- 1:5
a <- alias(lm(y ~ x))
cat(deparse(a$Model), "\n", sep = "")
cat(class(a), "\n", sep = "")
cat(names(a), "\n", sep = "")
