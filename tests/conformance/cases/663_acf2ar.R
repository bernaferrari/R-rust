d <- acf2AR(c(1, 0.5, 0.25))
cat(paste(sprintf("%.8f", as.vector(d)), collapse = ","), "\n", sep = "")
cat(paste(dim(d), collapse = ","), "\n", sep = "")
cat(paste(rownames(d), collapse = ","), "\n", sep = "")
