x <- array(1:24, c(2, 3, 4))
y <- aperm(x, c(2, 1, 3))
print(paste(as.vector(y), collapse = ","))
print(dim(y))

z <- aperm(x, c(3, 1, 2), resize = FALSE)
print(paste(as.vector(z), collapse = ","))
print(dim(z))
