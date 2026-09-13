x <- 1:6
y <- SSbiexp(x, 10, 1, 2, 0.2)
cat(paste(round(y, 6), collapse = ","), "\n", sep = "")
