d <- as.Date("2020-02-03")
print(d)
print(format(d))
print(as.character(d))
print(unclass(d))

x <- structure(18295, class = "Date")
print(x)
print(format(x))
print(as.character(x))

print(as.Date(18295, origin = "1970-01-01"))
print(as.Date(character(0)))
print(format(as.Date(character(0))))
