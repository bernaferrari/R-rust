d <- decompose(ts(1:24, frequency = 4))
cat(paste(sprintf("%.6f", as.vector(d$trend)[1:8]), collapse = ","), "\n", sep = "")
cat(class(d), "\n", sep = "")
