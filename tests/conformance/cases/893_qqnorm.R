q <- qqnorm(1:10, plot.it = FALSE)
cat(paste(round(q$x, 4), collapse = ","), "\n", sep = "")
cat(paste(q$y, collapse = ","), "\n", sep = "")
