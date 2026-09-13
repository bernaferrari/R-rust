cat(format.POSIXlt(as.POSIXlt("2020-01-01", tz = "GMT")), "\n", sep = "")
cat(format.POSIXlt(as.POSIXlt("2020-01-01", tz = "GMT"), "%Y-%m-%d"), "\n", sep = "")
cat(format.POSIXct(ISOdate(2020, 1, 1), "%Y-%m-%d"), "\n", sep = "")
cat(format.POSIXct(ISOdate(2020, 1, 1), "%H"), "\n", sep = "")
