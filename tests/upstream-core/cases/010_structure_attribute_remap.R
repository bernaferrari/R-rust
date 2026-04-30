## Curated from r-source/tests/structure.R:
## matrix attribute smoke coverage adjacent to structure() tests.
X <- matrix(1:4, 2, 2, dimnames = list(c("A", "B"), 1:2))
print(dim(X))

x <- structure(1:3, names = c("a", "b", "c"))
print(x)
print(names(x))
print(names(attributes(x)))
print(attr(x, "names"))

y <- structure(
  1:4,
  .Dim = c(2, 2),
  .Dimnames = list(c("r1", "r2"), c("c1", "c2"))
)
print(dim(y))
print(dimnames(y)[[1]])
print(dimnames(y)[[2]])

z <- structure(1:2, .Label = c("a", "b"), class = "factor")
print(z)
print(levels(z))
print(class(z))
