d <- density.default(1:10, bw = 1, n = 8)
cat(paste(round(d$y, 5), collapse = ","), "\n", sep = "")
