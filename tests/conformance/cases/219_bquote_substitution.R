x <- 2
print(deparse1(bquote(1 + .(x))))
print(deparse1(bquote(list(a = .(x), b = y))))
