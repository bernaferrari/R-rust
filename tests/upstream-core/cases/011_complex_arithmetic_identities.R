## Curated from r-source/tests/complex.R:
## complex literal parsing and arithmetic identities.
options(digits = 7)

print(1i)
print(0i)
print(1 + 2i)

z <- as.complex(c(1, 2)) + as.complex(c(3, 4))
print(z)
print(typeof(z))

z <- as.complex(c(1, 2)) * as.complex(c(3, 4))
print(z)
print(typeof(z))
