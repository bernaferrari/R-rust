cat(paste(as.vector(cycle(ts(1:8, frequency = 4))), collapse = ","), "\n", sep = "")
cat(paste(sprintf("%.8f", as.vector(time(ts(1:4, frequency = 4)))), collapse = ","), "\n", sep = "")
