cat(as.numeric(ISOdate(2020, 1, 1)), "\n", sep = "")
cat(attr(ISOdate(2020, 1, 1), "tzone"), "\n", sep = "")
cat(paste(class(ISOdate(2020, 1, 1)), collapse = ","), "\n", sep = "")
cat(as.numeric(ISOdatetime(2020, 1, 1, 12, 0, 0, tz = "GMT")), "\n", sep = "")
