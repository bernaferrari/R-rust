x <- 1:3
attr(x, "foo") <- "bar"
print(attr(x, "foo"))
print(names(attributes(x)))

attr(x, "foo") <- NULL
print(attr(x, "foo"))
print(is.null(attributes(x)))

attr(x, "names") <- c("a", "b", "c")
print(names(x))
