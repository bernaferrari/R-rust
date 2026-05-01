x <- 1:3
print(comment(x))

comment(x) <- "hi"
print(comment(x))
print(names(attributes(x)))

comment(x) <- NULL
print(comment(x))
print(is.null(attributes(x)))
