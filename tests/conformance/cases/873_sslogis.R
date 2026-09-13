x <- 1:8
y <- SSlogis(x, 10, 4, 1.5)
cat(paste(round(y, 6), collapse = ","), "\n", sep = "")
