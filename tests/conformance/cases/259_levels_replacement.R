f <- factor(c("a", "b", "a"))
levels(f) <- c("A", "B")
print(f)
print(levels(f))

g <- factor(c("a", "b"))
levels(g) <- list(A = "a", B = "b")
print(g)
print(levels(g))

h <- factor(c("a", "b"))
levels(h) <- c("A", NA)
print(unclass(h)[1])
print(unclass(h)[2])
print(is.na(unclass(h)[2]))
print(levels(h))

x <- 1:3
levels(x) <- c("a", "b", "c")
print(levels(x))
print(class(x))

print(tryCatch({
    z <- factor(c("a", "b"))
    levels(z) <- c("A")
    z
}, error = function(e) conditionMessage(e)))
