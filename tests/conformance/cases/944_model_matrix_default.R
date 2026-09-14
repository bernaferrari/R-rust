d <- data.frame(y = 1:4, x = c(1, 2, 3, 4))
cat(paste(as.vector(model.matrix.default(y ~ x, d)), collapse = ","), "\n", sep = "")
