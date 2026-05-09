f <- y ~ x
g <- ~ x

print(class(f))
print(is.call(f))
print(typeof(f))
print(is.environment(attr(f, ".Environment")))
print(class(g))
print(is.call(g))
print(typeof(g))
print(is.environment(attr(g, ".Environment")))
