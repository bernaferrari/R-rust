cat(as.numeric(quantile(1:10, 0.5)), "\n", sep = "")
cat(paste(as.numeric(quantile(1:10)), collapse = ","), "\n", sep = "")
cat(paste(names(quantile(1:10)), collapse = ","), "\n", sep = "")
cat(as.numeric(quantile(c(1, 2, 3, 4), 0.5)), "\n", sep = "")
