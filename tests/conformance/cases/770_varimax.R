L <- matrix(c(0.9, 0.3, 0.8, 0.4, 0.3, 0.9, 0.4, 0.8, 0.7, 0.6), 5, 2, byrow = TRUE)
v <- varimax(L)
cat(paste(round(as.vector(v$loadings), 4), collapse = ","), "\n", sep = "")
cat(paste(round(as.vector(v$rotmat), 4), collapse = ","), "\n", sep = "")
