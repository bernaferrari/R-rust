d <- decompose(ts(1:24, frequency = 4))
cat(d$type, "\n", sep = "")
cat(paste(as.vector(d$x)[1:3], collapse = ","), "\n", sep = "")
