s <- loess.smooth(1:5, c(1.1, 1.9, 3.2, 3.8, 5.1), span = 1, degree = 1, family = "gaussian", evaluation = 5)
cat(paste(round(s$y, 4), collapse = ","), "\n", sep = "")
