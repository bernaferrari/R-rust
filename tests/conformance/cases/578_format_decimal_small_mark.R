cat(paste(format(c(1.23456, 7.8), decimal.mark = ","), collapse = "|"), "\n", sep = "")
cat(paste(format(c(0.123456789), small.mark = " ", scientific = FALSE), collapse = "|"), "\n", sep = "")
cat(paste(format(c(1.23456789, 98.7654321), small.mark = ":", small.interval = 3, scientific = FALSE), collapse = "|"), "\n", sep = "")
