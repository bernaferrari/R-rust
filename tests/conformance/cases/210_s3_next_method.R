describe <- function(x) UseMethod("describe")
describe.default <- function(x) "default"
describe.bar <- function(x) "bar"
describe.foo <- function(x) paste("foo", NextMethod())

x <- 1L
class(x) <- c("foo", "bar")

print(describe(x))
