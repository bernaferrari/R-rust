d <- polyroot(c(1, 0, 1))
cat(all(abs(Re(d)) < 1e-8), "\n", sep = "")
cat(paste(sprintf("%.8f", sort(Im(d))), collapse = ","), "\n", sep = "")
