y <- c(1, 2, 3, 10, 11, 12)
f <- factor(c("a", "a", "a", "b", "b", "b"))
fit <- lm(y ~ f)
cat(paste(unlist(dummy.coef(fit)), collapse = ","), "\n", sep = "")
