x <- 1:6
f <- gl(2, 3)
u <- unsplit(split(x, f), f)
cat(paste(as.integer(u), collapse = ","), "\n", sep = "")
cat(identical(u, x), "\n", sep = "")
