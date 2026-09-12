r <- runmed(c(1, 2, 10, 2, 1), 3)
cat(paste(as.vector(r), collapse = ","), "\n", sep = "")
cat(attr(r, "k"), "\n", sep = "")
