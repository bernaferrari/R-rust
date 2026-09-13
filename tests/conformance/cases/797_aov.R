y <- c(1, 2, 2, 4, 5)
x <- 1:5
a <- aov(y ~ x)
cat(paste(round(as.numeric(coef(a)), 4), collapse = ","), "\n", sep = "")
cat(paste(class(a), collapse = ","), "\n", sep = "")
cat(paste(round(as.vector(as.matrix(anova(a))), 4), collapse = ","), "\n", sep = "")
