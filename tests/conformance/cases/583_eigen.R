d <- eigen(diag(2))
cat(paste(d$values, collapse = ","), "\n", sep = "")
cat(paste(dim(d$vectors), collapse = "x"), "\n", sep = "")
cat(inherits(d, "eigen"), "\n", sep = "")
ok <- TRUE
for (j in 1:2) {
  av <- (diag(2) %*% d$vectors[, j])[, 1]
  lv <- d$values[j] * d$vectors[, j]
  if (max(abs(av - lv)) > 1e-10) ok <- FALSE
}
cat(ok, "\n", sep = "")
d2 <- eigen(matrix(c(2, 1, 1, 2), 2, 2), only.values = TRUE)
cat(paste(sort(round(d2$values, 8), decreasing = TRUE), collapse = ","), "\n", sep = "")
cat(is.null(d2$vectors), "\n", sep = "")
