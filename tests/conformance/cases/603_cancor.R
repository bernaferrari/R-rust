d <- cancor(matrix(1:6, 3, 2), matrix(2:7, 3, 2))
cat(sprintf("%.8f", d$cor), "\n", sep = "")
cat(paste(dim(d$xcoef), collapse = "x"), "\n", sep = "")
cat(paste(dim(d$ycoef), collapse = "x"), "\n", sep = "")
