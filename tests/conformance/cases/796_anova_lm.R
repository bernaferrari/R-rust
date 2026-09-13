y <- c(1, 2, 2, 4, 5)
x <- 1:5
a <- anova(lm(y ~ x))
cat(paste(round(as.vector(as.matrix(a)), 4), collapse = ","), "\n", sep = "")
cat(paste(class(a), collapse = ","), "\n", sep = "")
