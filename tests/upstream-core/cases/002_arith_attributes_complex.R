## Curated from r-source/tests/arith.R and arith-true.R:
## arithmetic type promotion, recycling, attributes, and complex vectors.
options(digits = 7)

x <- c(a = 1L, b = 2147483647L, c = 3L)
y <- suppressWarnings(x + c(1L, 1L, 1L))
print(y[[1]])
print(is.na(y[[2]]))
print(y[[3]])
print(names(suppressWarnings(x + 1L)))
print(typeof(2L ^ 10L))

m <- matrix(1:4, nrow = 2)
dimnames(m) <- list(c("r1", "r2"), c("c1", "c2"))
print(m + 1)
print(dim(m + 1))
dn <- dimnames(m + 1)
print(dn[[1]])
print(dn[[2]])

z <- as.complex(c(1, 2)) + as.complex(c(3, 4))
print(z)
print(typeof(z))
