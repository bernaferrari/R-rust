x <- structure(1:2, foo = "bar", names = c("a", "b"))
storage.mode(x) <- "double"
print(unname(x[1]))
print(typeof(x))
print(attr(x, "foo"))
print(names(x))

y <- c(TRUE, FALSE)
storage.mode(y) <- "integer"
print(y)
print(typeof(y))

z <- c("1", "2")
storage.mode(z) <- "integer"
print(z)
print(typeof(z))

m <- matrix(1:4, 2)
storage.mode(m) <- "double"
print(m)
print(dim(m))

n <- NULL
storage.mode(n) <- "list"
print(n)

r <- 1:2
storage.mode(r) <- "raw"
print(r)
print(typeof(r))

print(tryCatch({
    bad <- 1:2
    storage.mode(bad) <- "real"
    bad
}, error = function(e) conditionMessage(e)))
print(tryCatch({
    bad <- 1:2
    storage.mode(bad) <- "bad"
    bad
}, error = function(e) conditionMessage(e)))
print(tryCatch({
    f <- factor(c("a", "b"))
    storage.mode(f) <- "double"
    f
}, error = function(e) conditionMessage(e)))
