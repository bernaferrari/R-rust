d <- HoltWinters(ts(1:20, frequency = 4))
cat(paste(as.vector(d$x)[1:3], collapse = ","), "\n", sep = "")
cat(length(d$x), "\n", sep = "")
