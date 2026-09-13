cat(as.numeric(as.POSIXct(as.POSIXlt("2020-01-01", tz = "GMT"), tz = "GMT")), "\n", sep = "")
cat(as.numeric(as.POSIXct(as.POSIXlt("2020-01-01 12:00:00", tz = "GMT"), tz = "GMT")), "\n", sep = "")
cat(paste(class(as.POSIXct(as.POSIXlt("2020-01-01", tz = "GMT"), tz = "GMT")), collapse = ","), "\n", sep = "")
