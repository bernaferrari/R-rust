cat(as.vector(adist("abc", "abd")), "\n", sep = "")
cat(paste(dim(adist("abc", "abd")), collapse = ","), "\n", sep = "")
cat(paste(as.vector(adist(c("abc", "ab"), c("abd", "a"))), collapse = ","), "\n", sep = "")
cat(as.vector(adist("kitten", "sitting")), "\n", sep = "")
