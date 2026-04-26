## Curated from r-source/tests/eval-etc.R:
## closure positional, named, default, lazy, and lexical evaluation.
f <- function(x, y = 10) x + y
print(f(1))
print(f(y = 2, x = 3))

lazy <- function(x, y) x
print(lazy(1, stop("unused")))

make_adder <- function(x) {
    function(y) x + y
}
add10 <- make_adder(10)
print(add10(5))

local_value <- 99
reader <- function() local_value
print(reader())
