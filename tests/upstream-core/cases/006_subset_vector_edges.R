## Curated from r-source/tests/array-subset.R and eval-etc.R:
## vector subsetting, NA propagation, negative exclusion, and [[ extraction.
x <- 1:5
print(x[c(TRUE, FALSE)])
print(x[c(TRUE, NA)])
print(x[c(1, 0, NA, 7)])
print(x[-c(2, 4)])

named <- c(alpha = 10, beta = 20, gamma = 30)
print(named[c("gamma", "alpha", "missing")])
print(names(named[c("gamma", "alpha")]))

lst <- list(alpha = 11, beta = 22)
print(lst[[2]])
print(lst[["beta"]])
print(lst$alpha)
