x <- c(1, 2, 3, 3, 4, 5, 6, 7, 8, 9)
cat(round(suppressWarnings(bw.bcv(x)), 1), "\n", sep = "")
