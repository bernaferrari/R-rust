d <- ccf(1:10, 10:1, plot = FALSE, lag.max = 2)
cat(paste(sprintf("%.8f", as.vector(d$acf)), collapse = ","), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
