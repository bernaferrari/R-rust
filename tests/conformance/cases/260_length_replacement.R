x <- 1:3
length(x) <- 5
print(x)
length(x) <- 2
print(x)

y <- c("a", "b")
length(y) <- 4
print(y[1])
print(y[3])
print(is.na(y[3]))

z <- list(1, 2)
length(z) <- 3
print(z[[1]])
print(is.null(z[[3]]))

n <- setNames(1:2, c("a", "b"))
length(n) <- 4
print(unname(n))
print(names(n)[1])
print(names(n)[3] == "")

cplx <- c(1+3i, 2+4i)
length(cplx) <- 4
print(cplx)

raw <- as.raw(1:2)
length(raw) <- 4
print(raw)

print(tryCatch({
    bad <- 1:3
    length(bad) <- NA
    bad
}, error = function(e) conditionMessage(e)))
