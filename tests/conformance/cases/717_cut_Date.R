cat(paste(as.character(cut.Date(as.Date(c("2020-01-01", "2020-01-10")), "week")), collapse = ","), "\n", sep = "")
cat(paste(as.character(cut.Date(as.Date(c("2020-01-15", "2020-02-20")), "month")), collapse = ","), "\n", sep = "")
cat(paste(as.character(cut.Date(as.Date(c("2020-01-15", "2021-03-01")), "year")), collapse = ","), "\n", sep = "")
cat(class(cut.Date(as.Date("2020-01-01"), "week")), "\n", sep = "")
