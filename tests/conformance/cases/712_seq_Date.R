cat(paste(strftime(seq.Date(as.Date("2020-01-01"), as.Date("2020-01-03"), by = 1), "%Y-%m-%d"), collapse = ","), "\n", sep = "")
cat(paste(as.numeric(seq.Date(as.Date("2020-01-01"), as.Date("2020-01-07"), by = 2)), collapse = ","), "\n", sep = "")
cat(paste(as.numeric(seq.Date(as.Date("2020-01-01"), by = 1, length.out = 3)), collapse = ","), "\n", sep = "")
cat(class(seq.Date(as.Date("2020-01-01"), as.Date("2020-01-03"), by = 1)), "\n", sep = "")
