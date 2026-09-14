q <- qqplot(c(5, 1, 3), c(1.1, 5.1, 3.2), plot.it = FALSE)
cat(paste(c(q$x, q$y), collapse = ","), "\n", sep = "")
