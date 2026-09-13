cat(paste(as.vector(as.ts(1:10)), collapse = ","), "\n", sep = "")
cat(paste(as.vector(window(ts(1:10), start = 3, end = 5)), collapse = ","), "\n", sep = "")
cat(paste(sprintf("%.8f", tsp(lag(ts(1:5), 1))), collapse = ","), "\n", sep = "")
cat(paste(as.vector(lag(ts(1:5), 1)), collapse = ","), "\n", sep = "")
