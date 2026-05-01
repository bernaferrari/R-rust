print(array())
print(typeof(array()))
print(dim(array()))

print(array(1:4, c(2, 2)))
print(is.array(array(1:4, c(2, 2))))

print(array(1:3, c(2, 2)))
print(array(integer(0), c(2, 2)))

print(array(1:4, c(2, 2), dimnames = list(c("r1", "r2"), c("c1", "c2"))))
