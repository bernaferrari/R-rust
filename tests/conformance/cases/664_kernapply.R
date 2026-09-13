k <- kernel("daniell", 1)
cat(paste(sprintf("%.8f", as.vector(k$coef)), collapse = ","), "\n", sep = "")
cat(class(k), "\n", sep = "")
cat(paste(sprintf("%.8f", kernapply(1:10, k)), collapse = ","), "\n", sep = "")
