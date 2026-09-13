cat(paste(labels(c(a = 1, b = 2)), collapse = ","), "\n", sep = "")
cat(paste(labels(1:3), collapse = ","), "\n", sep = "")
obj <- list(na.action = 2:3)
cat(paste(na.action(obj), collapse = ","), "\n", sep = "")
