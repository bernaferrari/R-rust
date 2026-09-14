f <- splinefunH(c(1, 2, 3), c(1, 4, 9), c(2, 4, 6))
cat(paste(round(f(c(1, 1.5, 2, 2.5, 3)), 4), collapse = ","), "\n", sep = "")
