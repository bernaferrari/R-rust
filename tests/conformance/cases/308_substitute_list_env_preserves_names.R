print(names(as.list(c(x = 1, y = 2))))
print(deparse(substitute(x + y, as.list(c(x = 1, y = 2)))))
print(deparse(substitute(x + y, list(x = 1, y = quote(foo)))))
