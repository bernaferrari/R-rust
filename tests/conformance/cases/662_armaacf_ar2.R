cat(paste(sprintf("%.10f", ARMAacf(ar = c(0.5, -0.1), lag.max = 3)), collapse = ","), "\n", sep = "")
