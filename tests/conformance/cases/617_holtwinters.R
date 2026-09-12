d <- HoltWinters(ts(1:24, frequency = 12))
cat(paste(dim(d$fitted), collapse = "x"), "\n", sep = "")
cat(paste(sprintf("%.4f", as.vector(d$fitted)[1:3]), collapse = ","), "\n", sep = "")
cat(paste(class(d), collapse = ","), "\n", sep = "")
