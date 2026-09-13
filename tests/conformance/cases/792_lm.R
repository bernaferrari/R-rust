y <- c(1, 2, 2, 4, 5)
x <- 1:5
fit <- lm(y ~ x)
cat(paste(round(as.numeric(coef(fit)), 4), collapse = ","), "\n", sep = "")
cat(paste(round(as.numeric(fitted(fit)), 4), collapse = ","), "\n", sep = "")
cat(paste(round(as.numeric(resid(fit)), 4), collapse = ","), "\n", sep = "")
cat(fit$rank, "\n", sep = "")
cat(fit$df.residual, "\n", sep = "")
cat(class(fit), "\n", sep = "")
