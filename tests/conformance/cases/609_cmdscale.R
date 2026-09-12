p <- cmdscale(dist(1:4), k = 1)
cat(paste(sprintf("%.8f", as.vector(p)), collapse = ","), "\n", sep = "")
cat(paste(dim(p), collapse = "x"), "\n", sep = "")
