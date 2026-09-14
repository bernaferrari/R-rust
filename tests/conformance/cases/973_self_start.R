ss <- selfStart(~ A * x, function(mCall, data, LHS, ...) list(A = 1), c("A"))
cat(paste(attr(ss, "pnames"), collapse = ","), "\n", sep = "")
