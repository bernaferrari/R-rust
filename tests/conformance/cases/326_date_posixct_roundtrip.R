p <- as.POSIXct("2020-02-03 04:05:06", tz = "UTC")
print(as.Date(p))
print(unclass(as.Date(p)))

print(as.Date(as.POSIXct("1969-12-31 23:59:59", tz = "UTC")))

d <- as.Date("2020-02-03")
print(as.POSIXct(d, tz = "UTC"))
