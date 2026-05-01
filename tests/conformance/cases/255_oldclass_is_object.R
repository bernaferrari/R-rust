x <- structure(1, class = c("foo", "bar"))
print(oldClass(x))
print(is.object(x))

oldClass(x) <- "baz"
print(class(x))
print(oldClass(x))
print(is.object(x))

oldClass(x) <- NULL
print(class(x))
print(oldClass(x))
print(is.object(x))

print(oldClass(1))
print(is.object(1))
print(isS4(1))
