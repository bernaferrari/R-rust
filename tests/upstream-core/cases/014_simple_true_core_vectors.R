## Curated from r-source/tests/simple-true.R:
## small vector, sequence, cumsum, rev, dimnames, and data-frame invariants.
print(all(1:12 == cumsum(rep(1, 12))))
print(typeof(1:4) == "integer")
print(typeof(1L) == "integer")
print(1 == as.integer(is.nan(0 / 0)))
