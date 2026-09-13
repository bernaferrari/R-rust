y <- c(1, 2, 2, 4, 5)
x <- 1:5
fit <- lm(y ~ x)
cat(paste(variable.names(fit), collapse = ","), "\n", sep = "")
cat(paste(case.names(fit), collapse = ","), "\n", sep = "")
