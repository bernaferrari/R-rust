cat(prettyNum(1234), "\n", sep = "")
cat(prettyNum(1234.5, big.mark = ","), "\n", sep = "")
cat(paste(prettyNum(c(12, 1234.5), big.mark = ","), collapse = ","), "\n", sep = "")
cat(prettyNum(1.234, decimal.mark = ";"), "\n", sep = "")
