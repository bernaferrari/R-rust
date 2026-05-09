x <- array(
  1:24,
  dim = c(2, 3, 4),
  dimnames = list(
    r = c("r1", "r2"),
    c = c("c1", "c2", "c3"),
    z = c("z1", "z2", "z3", "z4")
  )
)

cs <- colSums(x, dims = 1)
rs <- rowSums(x, dims = 2)
cm <- colMeans(x, dims = 1)
rm <- rowMeans(x, dims = 2)

cat(paste(as.vector(cs), collapse = "|"), "\n")
cat(paste(dim(cs), collapse = "|"), "\n")
cat(paste(dimnames(cs)[[1]], collapse = "|"), "\n")
cat(paste(dimnames(cs)[[2]], collapse = "|"), "\n")
cat(paste(as.vector(rs), collapse = "|"), "\n")
cat(paste(dim(rs), collapse = "|"), "\n")
cat(paste(dimnames(rs)[[1]], collapse = "|"), "\n")
cat(paste(dimnames(rs)[[2]], collapse = "|"), "\n")
cat(paste(as.vector(cm), collapse = "|"), "\n")
cat(paste(as.vector(rm), collapse = "|"), "\n")

y <- array(c(1, NA, 3, 4, NA, 6, 7, 8), dim = c(2, 2, 2))
cat(paste(as.vector(colSums(y, dims = 1)), collapse = "|"), "\n")
cat(paste(as.vector(colSums(y, dims = 1, na.rm = TRUE)), collapse = "|"), "\n")
cat(paste(as.vector(rowMeans(y, dims = 2, na.rm = TRUE)), collapse = "|"), "\n")
