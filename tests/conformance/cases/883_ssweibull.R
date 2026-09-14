x <- 1:6
y <- SSweibull(x, 10, 8, -1, 2)
cat(paste(round(y, 6), collapse = ","), "\n", sep = "")
