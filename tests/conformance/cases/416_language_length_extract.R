f <- y ~ x
g <- ~ x
h <- quote(foo(1, bar))

print(length(f))
print(length(g))
print(length(h))
print(as.character(f[[1]]))
print(as.character(f[[2]]))
print(as.character(f[[3]]))
print(as.character(g[[1]]))
print(as.character(g[[2]]))
print(as.character(h[[1]]))
print(h[[2]])
print(as.character(h[[3]]))
