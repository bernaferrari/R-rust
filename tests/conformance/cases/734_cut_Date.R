cat(paste(as.character(cut(as.Date(c("2020-01-01", "2020-06-01")), "quarter")), collapse = ","), "\n", sep = "")
cat(paste(levels(cut(as.Date(c("2020-01-01", "2020-06-01")), "quarter")), collapse = ","), "\n", sep = "")
cat(class(cut(as.Date(c("2020-01-01", "2020-06-01")), "quarter")), "\n", sep = "")
cat(paste(as.character(cut.Date(as.Date(c("2020-01-01", "2020-06-01")), "quarter")), collapse = ","), "\n", sep = "")
