## Curated from r-source/tests/arith-true.R:
## IEEE infinity/NaN checks and power identities.
options(digits = 7)

i1 <- 1 / 0
i2 <- 1:1 / 0:0
print(c(i1, i2, -i1, -i2))
print(i1 > 12)
print(i2 > 12)
print((-i1) < -12)
print((-i2) < -12)
print(1 / 0 == Inf)
print(0 ^ -1 == Inf)
print(1 / Inf == 0)
print(Inf ^ -1 == 0)

r <- c(-2, -1, 0, 1, 2, NA, NaN)
print(suppressWarnings(r ^ 0))
print(suppressWarnings(as.integer(r) ^ 0L))
