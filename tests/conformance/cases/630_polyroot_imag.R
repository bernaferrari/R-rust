d <- polyroot(c(1, 0, 1))
cat(paste(sprintf("%.8f", sort(Re(d))), collapse = ","), "\n", sep = "")
cat(paste(sprintf("%.8f", sort(Im(d))), collapse = ","), "\n", sep = "")
