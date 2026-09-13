x <- list(loadings = matrix(c(0.8, 0.2, 0.1, 0.9), 2, 2), rotmat = diag(2))
ld <- loadings(x)
cat(paste(as.vector(ld), collapse = ","), "\n", sep = "")
cat(paste(dim(ld), collapse = ","), "\n", sep = "")
