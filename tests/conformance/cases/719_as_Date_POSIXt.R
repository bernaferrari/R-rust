cat(as.numeric(as.Date.POSIXct(ISOdate(2020, 1, 1))), "\n", sep = "")
cat(as.numeric(as.Date.POSIXlt(as.POSIXlt("2020-01-01"))), "\n", sep = "")
cat(class(as.Date.POSIXlt(as.POSIXlt("2020-01-01"))), "\n", sep = "")
cat(as.numeric(as.Date(as.POSIXlt("2020-01-15"))), "\n", sep = "")
