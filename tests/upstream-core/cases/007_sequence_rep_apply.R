## Curated from r-source/tests/eval-etc.R and primitives.R:
## sequence generation, rep recycling, apply-family calls, and do.call.
print(seq_len(4))
print(seq_along(c("a", "b")))
print(sequence(c(3, 2), from = c(5, 10), by = c(2, -1)))

print(rep(c("a", "b"), each = 2))
print(rep(c(1, 2), times = c(2, 3)))

m <- matrix(1:4, nrow = 2)
print(apply(m, 1, sum))
print(sapply(list(1:2, 3:5), length))
print(vapply(list(1:2, 3:5), length, integer(1)))
print(do.call(sum, list(1, 2, 3)))
