cat(paste(as.numeric(seq.POSIXt(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3), by = "day")), collapse = ","), "\n", sep = "")
cat(paste(as.numeric(seq.POSIXt(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3), by = 86400)), collapse = ","), "\n", sep = "")
cat(paste(as.numeric(seq.POSIXt(ISOdate(2020, 1, 1), by = "day", length.out = 3)), collapse = ","), "\n", sep = "")
cat(paste(class(seq.POSIXt(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3), by = "day")), collapse = ","), "\n", sep = "")
