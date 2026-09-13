cat(as.numeric(mean.Date(as.Date(c("2020-01-01", "2020-01-03")))), "\n", sep = "")
cat(class(mean.Date(as.Date(c("2020-01-01", "2020-01-03")))), "\n", sep = "")
cat(as.numeric(mean.POSIXct(c(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3)))), "\n", sep = "")
cat(paste(class(mean.POSIXct(c(ISOdate(2020, 1, 1), ISOdate(2020, 1, 3)))), collapse = ","), "\n", sep = "")
