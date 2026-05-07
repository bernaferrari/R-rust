x <- structure(1:2, class = "foo", names = c("a", "b"), other = "kept")
print(unclass(x))

f <- factor(c("b", "a", "b"))
print(unclass(f))

r <- regexpr("a", "abc")
print(r)

y <- structure(1, a = 1, b = 2)
print(y)
