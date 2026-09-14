cat(paste(as.vector(loglin(matrix(c(10, 20, 30, 40), 2, 2), list(1, 2), fit = TRUE, print = FALSE)$fit), collapse = ","), "\n", sep = "")
