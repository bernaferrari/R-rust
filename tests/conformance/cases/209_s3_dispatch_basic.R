shape <- function(x) UseMethod("shape")
shape.default <- function(x) "default"
shape.foo <- function(x) paste("foo", x$value)

x <- list(value = 7L)
class(x) <- "foo"

print(shape(x))
print(shape(1L))
print(getS3method("shape", "foo")(x))
