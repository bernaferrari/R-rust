set.seed(1)
cat(paste(round(as.vector(rWishart(1, 4, diag(2))), 4), collapse = ","), "\n", sep = "")
