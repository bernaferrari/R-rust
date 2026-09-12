d <- acf(1:10, plot = FALSE, lag.max = 3)
cat(paste(sprintf("%.8f", as.vector(d$acf)), collapse = ","), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
