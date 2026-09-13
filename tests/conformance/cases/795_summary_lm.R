y <- c(1, 2, 2, 4, 5)
x <- 1:5
s <- summary(lm(y ~ x))
cat(paste(round(as.vector(coef(s)), 4), collapse = ","), "\n", sep = "")
cat(round(s$sigma, 4), "\n", sep = "")
cat(round(s$r.squared, 4), "\n", sep = "")
cat(round(s$adj.r.squared, 4), "\n", sep = "")
cat(round(as.numeric(s$fstatistic["value"]), 1), "\n", sep = "")
cat(class(s), "\n", sep = "")
