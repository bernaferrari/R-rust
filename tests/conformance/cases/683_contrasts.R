d <- contrasts(factor(c("a", "b", "c")))
cat(paste(sprintf("%.0f", as.vector(d)), collapse = ","), "\n", sep = "")
cat(paste(rownames(d), collapse = ","), "\n", sep = "")
cat(paste(colnames(d), collapse = ","), "\n", sep = "")
