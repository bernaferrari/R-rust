# Regression: deparse must discriminate [ from [[ from the subset call's
# funtab identity (upstream PRIMVAL(SYMVALUE(op))), instead of rendering
# every subset call with double brackets.
x <- c(10, 20, 30)
cat(deparse(quote(x[1])), "\n")
cat(deparse(quote(x[[1]])), "\n")
cat(deparse(quote(x[1, 2])), "\n")
m <- matrix(1:4, 2, 2)
cat(deparse(quote(m[2, 1])), "\n")
cat(deparse(quote(m[, 1])), "\n")
lst <- list(a = 1, b = 2)
cat(deparse(quote(lst$a)), "\n")
cat(deparse(quote(lst[["b"]])), "\n")
cat(deparse(quote(m[[1]][2])), "\n")
cat(deparse(quote(x[-1])), "\n")
cat(deparse(quote(f(x[1], g[[2]]))), "\n")
