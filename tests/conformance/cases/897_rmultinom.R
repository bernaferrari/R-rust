set.seed(1)
cat(paste(as.vector(rmultinom(1, 10, c(0.2, 0.3, 0.5))), collapse = ","), "\n", sep = "")
