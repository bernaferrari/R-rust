x <- setNames(1:4, c("a", "b", "c", "d"))
dim(x) <- c(2, 2)
print(x)
print(dim(x))
print(names(x))
print(dimnames(x))

dimnames(x) <- list(c("r1", "r2"), c("c1", "c2"))
dim(x) <- c(4, 1)
print(dim(x))
print(dimnames(x))

dim(x) <- NULL
print(x)
print(dim(x))

y <- 1:4
dim(y) <- c("2.5", "2")
print(dim(y))

print(tryCatch({
    z <- 1:4
    dim(z) <- c(3, 2)
    z
}, error = function(e) conditionMessage(e)))
print(tryCatch({
    z <- 1:4
    dim(z) <- c(2, NA)
    z
}, error = function(e) conditionMessage(e)))
print(tryCatch({
    z <- 1:4
    dim(z) <- list(2, 2)
    z
}, error = function(e) conditionMessage(e)))
