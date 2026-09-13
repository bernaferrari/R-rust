y <- c(1, 2, 2, 4, 5)
x <- 1:5
g <- glm(y ~ x)
cat(paste(round(as.numeric(coef(g)), 4), collapse = ","), "\n", sep = "")
cat(paste(class(g), collapse = ","), "\n", sep = "")
cat(paste(round(as.numeric(fitted(g)), 4), collapse = ","), "\n", sep = "")
