x <- 1:3
attributes(x) <- list(foo = "bar", names = c("a", "b", "c"))
print(attr(x, "foo"))
print(names(x))
print(names(attributes(x))[1])
print(names(attributes(x))[2])

attributes(x) <- NULL
print(is.null(attributes(x)))
print(x)
