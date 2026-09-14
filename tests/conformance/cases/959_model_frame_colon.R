d <- data.frame(y = 1:4, a = 1:4, b = 5:8)
cat(paste(names(model.frame(y ~ a:b, d)), collapse = ","), "\n", sep = "")
