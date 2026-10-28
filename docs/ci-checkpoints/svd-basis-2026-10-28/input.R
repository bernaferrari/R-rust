hilbert <- function(n) { i <- 1:n; 1 / outer(i - 1, i, "+") }
cases <- list(
  hilbert_rect = hilbert(9)[, 1:6],
  square_distinct = diag(c(4, 2, 1)),
  rank_deficient = rbind(c(1, 2, 3), c(2, 4, 6), c(1, 0, 1), c(3, 6, 9)),
  repeated = diag(c(3, 3, 1)),
  repeated_rect = rbind(diag(3), diag(3)),
  zero_rank = matrix(0, 3, 2)
)
for (name in names(cases)) {
  X <- cases[[name]]
  s <- svd(X)
  cat("CASE", name, paste(dim(X), collapse="x"), "\n")
  cat("d:"); print(signif(s$d, 8))
  cat("u:"); print(round(s$u, 5))
  cat("v:"); print(round(s$v, 5))
  cat("reconstruction:", max(abs(X - s$u %*% diag(s$d) %*% t(s$v))), "\n")
  cat("u-orthogonality:", max(abs(crossprod(s$u) - diag(ncol(s$u)))), "\n")
  cat("v-orthogonality:", max(abs(crossprod(s$v) - diag(ncol(s$v)))), "\n")
}
