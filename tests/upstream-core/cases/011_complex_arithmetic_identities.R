## Curated from r-source/tests/complex.R:
## complex arithmetic identities expressible without complex literal syntax.
options(digits = 7)

z <- as.complex(c(1, 2)) + as.complex(c(3, 4))
print(z)
print(typeof(z))

z <- as.complex(c(1, 2)) * as.complex(c(3, 4))
print(z)
print(typeof(z))
