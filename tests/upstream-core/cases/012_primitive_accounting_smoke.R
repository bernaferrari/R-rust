## Curated from r-source/tests/primitives.R:
## primitive classification smoke checks.
print(is.primitive(sum))
print(is.primitive(length))
print(is.primitive(c))
print(is.primitive(function(x) x))
print(typeof(sum))
print(typeof(length))
print(typeof(c))
print(is.primitive(base::sum))
print(is.primitive(base::length))
print(typeof(base::sum))
print(base::length(c(1, 2, 3)))
