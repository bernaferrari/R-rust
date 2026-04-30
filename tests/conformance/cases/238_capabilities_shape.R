x <- capabilities()
print(length(x))
print(is.logical(x))
print(names(x)[1])
print(names(x)[19])
print(!is.na(x[["sockets"]]))

