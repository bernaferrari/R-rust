x <- matrix(1:6, 2, 3)
y <- matrix(1:6, 3, 2)
print(x %*% y)
print(crossprod(x))
print(tcrossprod(x))

m <- matrix(c(1, 3, 2, 3, 2, 1), 2, 3)
print(max.col(m, ties.method = "first"))
print(max.col(m, ties.method = "last"))
