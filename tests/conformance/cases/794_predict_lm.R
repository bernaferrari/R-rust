y <- c(1, 2, 2, 4, 5)
x <- 1:5
fit <- lm(y ~ x)
cat(paste(round(as.numeric(predict(fit)), 4), collapse = ","), "\n", sep = "")
cat(paste(round(as.numeric(predict(fit, newdata = data.frame(x = c(0, 6)))), 4), collapse = ","), "\n", sep = "")
