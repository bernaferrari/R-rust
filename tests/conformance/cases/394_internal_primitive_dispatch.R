print(.Internal(mean(1:3)))
print(.Internal(mean(c(1, NA))))
print(typeof(.Primitive("sum")))
print(is.primitive(.Primitive("sum")))
