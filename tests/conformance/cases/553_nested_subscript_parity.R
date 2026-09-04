# Nested subscripts: a call result subscripted inside an outer subscript.
# Regression: the `]]` lexer fused adjacent closes, stranding the outer `]`.
v <- 1:3
cat("nested-call:", v[c(1,2)[1]], "\n")
cat("nested-double:", v[c(1,2)[[1]]], "\n")
m <- matrix(1:4, 2)
cat("matrix-dbl:", m[[3]], "\n")
l <- list(a = 1)
cat("list-name:", l[["a"]], "\n")
cat("outer-double:", v[[c(1,2)[1]]], "\n")
