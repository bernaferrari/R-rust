## Curated from r-source/tests/eval-etc.R and reg-tests-*.R:
## data-frame dimensions, with() evaluation, factor labels, and tables.
d <- data.frame(a = 1:3, b = c("x", "y", "z"))
print(dim(d))
print(names(d))
print(d["a"])
print(with(d, a + 1))

f <- factor(c("a", "b", "a", NA), levels = c("a", "b", NA))
print(as.character(f))
print(table(f, useNA = "ifany"))
