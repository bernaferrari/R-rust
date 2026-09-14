aliased <- c(FALSE, TRUE, FALSE)
vc <- matrix(c(1, 0.2, 0.2, 2), 2, 2)
cat(paste(as.vector(.vcov.aliased(aliased, vc)), collapse = ","), "\n", sep = "")
