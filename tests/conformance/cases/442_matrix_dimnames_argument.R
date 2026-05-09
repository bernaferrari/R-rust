m <- matrix(1:4, nrow = 2, dimnames = list(c("r1", "r2"), c("c1", "c2")))
cat(paste(dimnames(m)[[1]], collapse = "|"), "\n", sep = "")
cat(paste(dimnames(m)[[2]], collapse = "|"), "\n", sep = "")
cat(paste(as.vector(m), collapse = "|"), "\n", sep = "")
