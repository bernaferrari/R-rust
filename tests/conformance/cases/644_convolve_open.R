d <- convolve(1:3, 1:2, type = "open")
cat(paste(sprintf("%.8f", d), collapse = ","), "\n", sep = "")
e <- convolve(1:3, 3:1)
cat(paste(sprintf("%.8f", e), collapse = ","), "\n", sep = "")
