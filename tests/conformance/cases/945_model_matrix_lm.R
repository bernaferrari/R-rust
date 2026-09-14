fit <- lm(c(1.1, 1.9, 3.2, 3.8, 5.1) ~ I(1:5))
cat(paste(as.vector(model.matrix.lm(fit)), collapse = ","), "\n", sep = "")
