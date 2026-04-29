## Curated from r-source/tests/structure.R:
## matrix attribute smoke coverage adjacent to structure() tests.
X <- matrix(1:4, 2, 2, dimnames = list(c("A", "B"), 1:2))
print(dim(X))
