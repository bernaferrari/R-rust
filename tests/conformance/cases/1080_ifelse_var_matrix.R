cat(typeof(ifelse(TRUE, 1 + 0i, 0 + 0i)), "\n", sep = "")
cat(paste(Re(ifelse(c(TRUE, FALSE), 1 + 2i, 3 + 4i)), collapse = ","), "\n", sep = "")
cat(paste(dim(var(diag(3))), collapse = ","), "\n", sep = "")
cat(isTRUE(all.equal(
  3 * 2 * var(diag(3)),
  matrix(c(rep(c(2, rep(-1, 3)), 2), 2), nrow = 3, ncol = 3),
  tolerance = 20 * .Machine$double.eps
)), "\n", sep = "")
