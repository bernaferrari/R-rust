cat(as.numeric(julian(as.Date("2020-01-01"))), "\n", sep = "")
cat(as.numeric(attr(julian(as.Date("2020-01-01")), "origin")), "\n", sep = "")
cat(as.numeric(julian(as.Date("1970-01-01"))), "\n", sep = "")
cat(paste(as.numeric(julian(as.Date(c("2020-01-01", "2020-01-02")))), collapse = ","), "\n", sep = "")
