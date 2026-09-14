y <- 1:4
x <- 1:4
z <- c(10, 20, 30, 40)
fit <- lm(y ~ x)
cat(paste(expand.model.frame(fit, ~ z)$z, collapse = ","), "\n", sep = "")
