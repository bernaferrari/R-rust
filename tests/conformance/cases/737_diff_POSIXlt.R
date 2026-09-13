x <- as.POSIXlt(c(ISOdate(2020, 1, 1), ISOdate(2020, 1, 2)))
cat(as.numeric(diff(x)), "\n", sep = "")
cat(class(diff(x)), "\n", sep = "")
cat(units(diff(x)), "\n", sep = "")
