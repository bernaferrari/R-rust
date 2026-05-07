Sys.setenv(TZ = "UTC")

print(seq(as.Date("2020-02-01"), as.Date("2020-02-03"), by = "day"))
print(class(seq(as.Date("2020-02-01"), as.Date("2020-02-03"), by = "day")))
print(seq(as.Date("2020-02-03"), by = "week", length.out = 2))
print(class(seq(as.Date("2020-02-03"), by = "week", length.out = 2)))

p <- as.POSIXct("2020-02-03 00:00:00", tz = "UTC")
print(seq(p, by = "hour", length.out = 2))
print(class(seq(p, by = "hour", length.out = 2)))
print(attr(seq(p, by = "hour", length.out = 2), "tzone"))
