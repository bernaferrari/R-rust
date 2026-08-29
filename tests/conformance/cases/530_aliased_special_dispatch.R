# Regression: special-form dispatch must resolve through the bound function
# value (upstream dispatches on the primitive object's funtab entry), so an
# aliased special like `h <- `[`` works instead of failing on the head name.
x <- c(10, 20, 30)
h <- `[`
cat(h(x, 2), "\n")
g <- `[[`
lst <- list(a = 5, b = 6)
cat(g(lst, "a"), "\n")
d <- `$`
cat(d(lst, "b"), "\n")
p <- `(`
cat(p(2 + 3), "\n")
i2 <- `if`
cat(i2(TRUE, 5, 6), "\n")
w <- `while`
k <- 0
w(k < 3, k <- k + 1)
cat(k, "\n")
cat(deparse(quote(h(x, 2))), "\n")
