d <- convolve(1:4, 1:2, type = "filter")
cat(paste(sprintf("%.8f", d), collapse = ","), "\n", sep = "")
