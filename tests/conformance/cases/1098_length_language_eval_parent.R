cat(length(quote(B(a = 1, b = 2))), "\n", sep = "")
stopifnot(isTRUE(all.equal(
    as.list(quote(B(a = 1))),
    list(as.name("B"), a = 1)
)))
f <- function(x) eval.parent(substitute(x))
g <- function() {
    y <- 7L
    f(y)
}
cat(g(), "\n", sep = "")
