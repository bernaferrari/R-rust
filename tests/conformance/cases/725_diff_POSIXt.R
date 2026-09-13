cat(as.numeric(diff.POSIXt(c(ISOdate(2020, 1, 1), ISOdate(2020, 1, 2)))), "\n", sep = "")
cat(units(diff.POSIXt(c(ISOdate(2020, 1, 1), ISOdate(2020, 1, 2)))), "\n", sep = "")
cat(class(diff.POSIXt(c(ISOdate(2020, 1, 1), ISOdate(2020, 1, 2)))), "\n", sep = "")
cat(paste(as.numeric(diff.POSIXt(c(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3), ISOdate(2020, 1, 6)))), collapse = ","), "\n", sep = "")
