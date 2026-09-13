cat(is.ts(ts(1:5)), "\n", sep = "")
cat(is.ts(1:5), "\n", sep = "")
cat(HoltWinters(ts(1:20, frequency = 4))$seasonal, "\n", sep = "")
