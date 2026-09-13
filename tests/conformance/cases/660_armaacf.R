cat(paste(sprintf("%.8f", ARMAacf(ar = 0.5, lag.max = 3)), collapse = ","), "\n", sep = "")
cat(paste(names(ARMAacf(ar = 0.5, lag.max = 3)), collapse = ","), "\n", sep = "")
cat(paste(sprintf("%.8f", ARMAtoMA(ar = 0.5, lag.max = 3)), collapse = ","), "\n", sep = "")
