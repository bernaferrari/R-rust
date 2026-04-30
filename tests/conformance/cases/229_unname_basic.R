x <- c(a = TRUE, b = FALSE)
y <- unname(x)
print(y)
print(names(y))
z <- unname(list(a = 1, b = 2))
print(names(z))
print(z[[2]])

