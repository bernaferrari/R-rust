cat(paste(as.character(c.Date(as.Date("2020-01-01"), as.Date("2020-01-03"))), collapse = ","), "\n", sep = "")
cat(class(c.Date(as.Date("2020-01-01"), as.Date("2020-01-03"))), "\n", sep = "")
cat(paste(as.numeric(c.POSIXct(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3))), collapse = ","), "\n", sep = "")
cat(paste(class(c.POSIXct(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3))), collapse = ","), "\n", sep = "")
