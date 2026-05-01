m <- array(1:4, c(1, 4), dimnames = list("r1", c("a", "b", "c", "d")))
print(drop(m))
print(names(drop(m)))

a <- array(1:4, c(1, 2, 2), dimnames = list("r", c("a", "b"), c("x", "y")))
print(drop(a))
print(dim(drop(a)))
print(dimnames(drop(a))[[1]])
print(dimnames(drop(a))[[2]])

b <- array(1:4, c(2, 1, 2), dimnames = list(c("r1", "r2"), "only", c("x", "y")))
print(drop(b))
print(dimnames(drop(b))[[1]])
print(dimnames(drop(b))[[2]])

z <- array(42, c(1, 1), dimnames = list("r", "c"))
print(drop(z))
print(attributes(drop(z)))
